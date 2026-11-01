//! Durable document-processing pipeline. Every stage is persisted before the
//! next one starts, so a worker crash is recoverable from the processing job
//! and document state. AI output is never trusted: it is shape-checked,
//! validated, and only then persisted.

use chrono::Utc;
use invoxa_database::invoices::NewInvoice;
use invoxa_database::{auth, documents, invoices, jobs, operations, vendors, Db, DbError};
use invoxa_documents::{
    storage::{ObjectStorage, StorageError},
    text::{TextError, TextExtractionConfig, TextExtractor},
    validation::{validate_upload, ValidationError},
};
use invoxa_domain::{
    automation::{evaluate_rule, AutomationContext, AutomationOutcome, AutomationSpec},
    duplicate::{detect_duplicates, duplicate_status_label, CandidateInvoice, ExistingInvoice},
    invoice::ExtractedInvoice,
    normalization::{normalize_invoice_number, parse_invoice_date, vendor_match_key},
    status::DocumentStatus,
    validation::{
        validate_invoice, Severity, ValidationContext, ValidationIssue, ValidationPolicy,
    },
};
use invoxa_extraction::{ExtractionError, ExtractionInput, ExtractionProvider};
use rand::Rng;
use rust_decimal::{prelude::FromPrimitive, Decimal};
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Instant;
use tracing::{error, info, warn};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct ProcessingOutcome {
    pub job_id: Uuid,
    pub document_id: Uuid,
    pub status: &'static str,
    pub invoice_id: Option<Uuid>,
    pub message: Option<String>,
    pub retry_in_seconds: Option<i64>,
}

struct PipelineSuccess {
    invoice_id: Uuid,
    review_status: String,
}

#[derive(Debug)]
struct PipelineFailure {
    code: &'static str,
    message: String,
    retryable: bool,
}

impl PipelineFailure {
    fn terminal(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            retryable: false,
        }
    }

    fn retryable(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            retryable: true,
        }
    }
}

pub struct DocumentPipeline {
    db: Db,
    storage: Arc<dyn ObjectStorage>,
    extractor: Arc<TextExtractor>,
    provider: Arc<dyn ExtractionProvider>,
    text_config: TextExtractionConfig,
}

impl DocumentPipeline {
    pub fn new(
        db: Db,
        storage: Arc<dyn ObjectStorage>,
        extractor: Arc<TextExtractor>,
        provider: Arc<dyn ExtractionProvider>,
        text_config: TextExtractionConfig,
    ) -> Self {
        Self {
            db,
            storage,
            extractor,
            provider,
            text_config,
        }
    }

    pub fn provider_name(&self) -> &str {
        self.provider.name()
    }

    pub fn provider_is_mock(&self) -> bool {
        self.provider.is_mock()
    }

    pub fn db(&self) -> &Db {
        &self.db
    }

    /// Process one job that was already atomically claimed by `jobs::claim_job`.
    /// This method owns the terminal job-state update and never panics on
    /// malformed document content.
    pub async fn process_claimed(
        &self,
        job: invoxa_database::jobs::ClaimedJobRow,
    ) -> ProcessingOutcome {
        let job_id = job.job_id;
        let document_id = job.document_id;
        match self.run_once(&job).await {
            Ok(success) => {
                if let Err(error) = jobs::complete_job(&self.db, job_id, "completed").await {
                    error!(job_id = %job_id, error = %error, "job completed but status update failed");
                    return ProcessingOutcome {
                        job_id,
                        document_id,
                        status: "completion_failed",
                        invoice_id: Some(success.invoice_id),
                        message: Some(
                            "processing succeeded but the job status could not be saved".into(),
                        ),
                        retry_in_seconds: None,
                    };
                }
                info!(
                    job_id = %job_id,
                    document_id = %document_id,
                    invoice_id = %success.invoice_id,
                    review_status = %success.review_status,
                    "document processing succeeded"
                );
                ProcessingOutcome {
                    job_id,
                    document_id,
                    status: "succeeded",
                    invoice_id: Some(success.invoice_id),
                    message: Some(success.review_status),
                    retry_in_seconds: None,
                }
            }
            Err(failure) => self.record_failure(&job, failure).await,
        }
    }

    async fn record_failure(
        &self,
        job: &invoxa_database::jobs::ClaimedJobRow,
        failure: PipelineFailure,
    ) -> ProcessingOutcome {
        let retry = failure.retryable && job.attempts < job.max_attempts;
        let message = failure.message.chars().take(900).collect::<String>();
        if retry {
            let delay = retry_delay_seconds(job.attempts);
            let run_at = Utc::now() + chrono::Duration::seconds(delay);
            let rescheduled =
                jobs::reschedule_job(&self.db, job.job_id, run_at, failure.code, &message).await;
            if let Err(error) = rescheduled {
                error!(job_id = %job.job_id, error = %error, "could not reschedule failed job");
                let _ = jobs::fail_job(&self.db, job.job_id, true, failure.code, &message).await;
            }
            self.event(
                job.organization_id,
                job.document_id,
                "retry",
                "retry",
                Some(format!(
                    "{}: {} Retry scheduled in {} seconds.",
                    failure.code, message, delay
                )),
                None,
            )
            .await;
            info!(
                job_id = %job.job_id,
                code = failure.code,
                retry_in_seconds = delay,
                "document processing will retry"
            );
            ProcessingOutcome {
                job_id: job.job_id,
                document_id: job.document_id,
                status: "retry",
                invoice_id: None,
                message: Some(format!("{}: {}", failure.code, message)),
                retry_in_seconds: Some(delay),
            }
        } else {
            let _ = documents::set_processing_error(
                &self.db,
                job.organization_id,
                job.document_id,
                failure.code,
                &message,
            )
            .await;
            self.event(
                job.organization_id,
                job.document_id,
                "failed",
                "failed",
                Some(format!("{}: {}", failure.code, message)),
                None,
            )
            .await;
            self.fail_document(job.organization_id, job.document_id)
                .await;
            if let Err(error) =
                jobs::fail_job(&self.db, job.job_id, false, failure.code, &message).await
            {
                error!(job_id = %job.job_id, error = %error, "could not mark job dead");
            }
            if let Err(error) = crate::events::emit_document_event(
                &self.db,
                job.organization_id,
                crate::events::EVENT_DOCUMENT_PROCESSING_FAILED,
                job.document_id,
                job.attempts,
                Some(json!({
                    "error_code": failure.code,
                    "error_message": message.clone(),
                })),
            )
            .await
            {
                error!(
                    job_id = %job.job_id,
                    error = %error,
                    "could not queue document.processing_failed webhook"
                );
            }
            info!(
                job_id = %job.job_id,
                code = failure.code,
                "document processing failed permanently"
            );
            ProcessingOutcome {
                job_id: job.job_id,
                document_id: job.document_id,
                status: "dead",
                invoice_id: None,
                message: Some(format!("{}: {}", failure.code, message)),
                retry_in_seconds: None,
            }
        }
    }

    async fn fail_document(&self, organization_id: Uuid, document_id: Uuid) {
        let Ok(Some(document)) =
            documents::get_document(&self.db, organization_id, document_id).await
        else {
            return;
        };
        let Some(current) = DocumentStatus::parse(&document.processing_status) else {
            return;
        };
        if current.can_transition_to(DocumentStatus::Failed) {
            let _ = documents::transition_document(
                &self.db,
                organization_id,
                document_id,
                DocumentStatus::Failed,
                "failed",
                Some("Processing failed permanently. A user can retry the document."),
                None,
            )
            .await;
        }
    }

    async fn run_once(
        &self,
        job: &invoxa_database::jobs::ClaimedJobRow,
    ) -> Result<PipelineSuccess, PipelineFailure> {
        let total_started = Instant::now();
        let organization_id = job.organization_id;
        let document_id = job.document_id;

        let document = documents::get_document(&self.db, organization_id, document_id)
            .await
            .map_err(|e| sql_failure("load document", e))?
            .ok_or_else(|| {
                PipelineFailure::terminal("DOCUMENT_MISSING", "The document no longer exists.")
            })?;

        if matches!(
            document.processing_status.as_str(),
            "approved" | "exported" | "rejected" | "cancelled"
        ) {
            return Err(PipelineFailure::terminal(
                "DOCUMENT_STATE_INVALID",
                "The document is no longer eligible for automated processing.",
            ));
        }

        if document.processing_status != "queued" {
            documents::reset_document_for_retry(&self.db, organization_id, document_id)
                .await
                .map_err(|e| db_failure("restart document", e))?;
        }

        let document = documents::transition_document(
            &self.db,
            organization_id,
            document_id,
            DocumentStatus::ExtractingText,
            "text_extraction",
            Some("Starting text extraction from the stored document."),
            None,
        )
        .await
        .map_err(|e| db_failure("start text extraction", e))?;

        let organization = auth::organization_by_id(&self.db, organization_id)
            .await
            .map_err(|e| sql_failure("load organization", e))?
            .ok_or_else(|| {
                PipelineFailure::terminal(
                    "ORGANIZATION_MISSING",
                    "The organization no longer exists.",
                )
            })?;

        // External AI processing is an explicit organization-level consent
        // decision. The deterministic mock provider runs locally and does not
        // send document text anywhere, so it does not require this consent.
        let external_ai = !self.provider.is_mock();
        if external_ai
            && organization
                .settings
                .pointer("/ai/provider_consent")
                .and_then(Value::as_bool)
                != Some(true)
        {
            return Err(PipelineFailure::terminal(
                "AI_CONSENT_REQUIRED",
                "External AI processing is disabled for this organization. An owner must enable it in settings after reviewing the data-processing terms.",
            ));
        }

        // Fetch into a private temporary file so the OCR/PDF libraries never
        // read directly from a client-controlled path.
        let read_limit = usize::try_from(self.text_config.limits.max_file_bytes)
            .unwrap_or(usize::MAX)
            .saturating_add(1);
        let bytes = self
            .storage
            .get_bytes(&document.storage_key, read_limit)
            .await
            .map_err(storage_failure)?;
        let temp = tempfile::Builder::new()
            .prefix("invoxa-processing-")
            .suffix(".upload")
            .tempfile()
            .map_err(|e| {
                warn!(error = %e, "could not create processing temp file");
                PipelineFailure::retryable(
                    "PROCESSING_IO_ERROR",
                    "Could not create a temporary processing file.",
                )
            })?;
        tokio::fs::write(temp.path(), &bytes).await.map_err(|e| {
            warn!(error = %e, "could not write processing temp file");
            PipelineFailure::retryable(
                "PROCESSING_IO_ERROR",
                "Could not write a temporary processing file.",
            )
        })?;

        let preflight = validate_upload(
            temp.path(),
            &document.original_filename,
            &self.text_config.limits,
        )
        .await
        .map_err(upload_failure)?;

        let _ = documents::set_preflight_metadata(
            &self.db,
            organization_id,
            document_id,
            preflight.page_count as i32,
            &document.document_type,
        )
        .await;
        self.event(
            organization_id,
            document_id,
            "preflight",
            "passed",
            Some(format!(
                "File verified as {} with {} page(s).{}",
                preflight.content_type,
                preflight.page_count,
                if preflight.extension_mismatch {
                    " The filename extension did not match the detected format; detected content was used."
                } else {
                    ""
                }
            )),
            None,
        )
        .await;

        let text_started = Instant::now();
        let extracted_text = self
            .extractor
            .extract(temp.path(), preflight.kind)
            .await
            .map_err(text_failure)?;
        let text_duration = text_started.elapsed().as_millis() as i64;

        if extracted_text.ocr_pages > 0 {
            documents::transition_document(
                &self.db,
                organization_id,
                document_id,
                DocumentStatus::RunningOcr,
                "ocr",
                Some(&format!(
                    "OCR processed {} page(s) with Tesseract.{}",
                    extracted_text.ocr_pages,
                    extracted_text
                        .quality
                        .map(|q| format!(" Mean confidence: {:.1}%.", q * 100.0))
                        .unwrap_or_default()
                )),
                Some(text_duration),
            )
            .await
            .map_err(|e| db_failure("record OCR stage", e))?;
        }
        let document = documents::transition_document(
            &self.db,
            organization_id,
            document_id,
            DocumentStatus::ExtractingFields,
            "field_extraction",
            Some("Structured fields are being extracted from the document text."),
            None,
        )
        .await
        .map_err(|e| db_failure("start field extraction", e))?;

        let extraction_started = Instant::now();
        let result = self
            .provider
            .extract(ExtractionInput {
                filename: document.original_filename.clone(),
                country: organization.country.clone(),
                base_currency: organization.base_currency.clone(),
                pages: extracted_text
                    .pages
                    .iter()
                    .map(|page| invoxa_extraction::PageTextInput {
                        page_number: page.page_number,
                        text: page.text.clone(),
                    })
                    .collect(),
                used_ocr: extracted_text.ocr_pages > 0,
                ocr_quality: extracted_text.quality,
            })
            .await
            .map_err(extraction_failure)?;
        let extraction_duration = extraction_started.elapsed().as_millis() as i64;
        let is_mock = result.is_mock || self.provider.is_mock();

        if let Err(error) = documents::set_extraction_meta(
            &self.db,
            organization_id,
            document_id,
            &result.provider,
            &result.model,
            extraction_duration,
        )
        .await
        {
            error!(document_id = %document_id, error = %error, "could not store extraction metadata");
        }

        let extracted = &result.invoice;
        if let Err(errors) = extracted.validate_shape() {
            return Err(PipelineFailure::terminal(
                "AI_OUTPUT_INVALID",
                format!(
                    "The extraction provider returned data outside the accepted shape: {}",
                    errors.join("; ")
                ),
            ));
        }

        let policy = validation_policy(&organization);
        let validation = validate_invoice(
            extracted,
            &ValidationContext {
                policy,
                extraction_quality: extracted.extraction_quality.or(extracted_text.quality),
            },
        );
        let mut issues = validation.issues;

        // Surface text/provider diagnostics as informational issues. They are
        // bounded and deduplicated so a malformed provider cannot flood the DB.
        let mut seen_messages = BTreeSet::new();
        for warning in extracted_text
            .warnings
            .iter()
            .chain(result.warnings.iter())
            .chain(extracted.warnings.iter())
        {
            let message = warning.trim();
            if message.is_empty() || !seen_messages.insert(message.to_string()) {
                continue;
            }
            if seen_messages.len() > 20 {
                break;
            }
            issues.push(ValidationIssue::new(
                "EXTRACTION_WARNING",
                Severity::Info,
                None,
                message.chars().take(500).collect::<String>(),
            ));
        }

        if is_mock {
            issues.push(ValidationIssue::new(
                "MOCK_EXTRACTION",
                Severity::Warning,
                Some("extraction_provider"),
                "This invoice was extracted by the deterministic mock provider. The values are synthetic test data, not a real supplier document. Review and override explicitly before approval.",
            ));
        }

        if let Some(minimum) = minimum_ocr_quality(&organization.settings) {
            if extracted_text.ocr_pages > 0 {
                if let Some(quality) = extracted_text.quality {
                    if Decimal::from_f64(quality)
                        .map(|q| q < minimum)
                        .unwrap_or(false)
                    {
                        issues.push(ValidationIssue::new(
                            "LOW_OCR_QUALITY",
                            Severity::Warning,
                            None,
                            format!(
                                "OCR confidence {:.1}% is below the configured minimum {:.1}%. Verify values against the source document.",
                                quality * 100.0,
                                minimum * Decimal::from(100)
                            ),
                        ));
                    }
                }
            }
        }

        // Duplicate detection happens before the invoice is inserted, so its
        // final triage state can account for every open candidate. The
        // document's previous unapproved extraction (from a retry) must never
        // be treated as a duplicate of itself.
        let existing_for_document =
            invoices::find_invoice_by_document(&self.db, organization_id, document_id)
                .await
                .map_err(|e| sql_failure("load existing invoice", e))?;
        let exclude_invoice = existing_for_document
            .as_ref()
            .map(|invoice| invoice.id)
            .unwrap_or_else(Uuid::nil);
        let existing_rows = invoices::fingerprints(&self.db, organization_id, exclude_invoice)
            .await
            .map_err(|e| sql_failure("load duplicate fingerprints", e))?;
        let existing = existing_rows
            .iter()
            .map(|row| ExistingInvoice {
                id: row.id,
                file_hash: row.file_hash.clone(),
                vendor_key: row.vendor_name.as_deref().map(vendor_match_key),
                invoice_number_key: row
                    .invoice_number
                    .as_deref()
                    .map(normalize_invoice_number)
                    .filter(|value| !value.is_empty()),
                currency: row.currency.clone(),
                total: row.total,
                invoice_date: row.invoice_date,
            })
            .collect::<Vec<_>>();
        let parsed_invoice_date = extracted
            .invoice_date
            .as_deref()
            .and_then(|value| parse_invoice_date(value).ok())
            .map(|parsed| parsed.date);
        let candidate_vendor_key = extracted.vendor_match_key();
        let candidate_number_key = extracted.invoice_number_key();
        let candidate_currency = extracted.normalized_currency();
        let duplicate_matches = detect_duplicates(
            &CandidateInvoice {
                file_hash: Some(document.sha256.as_str()),
                vendor_key: candidate_vendor_key.as_deref(),
                invoice_number_key: candidate_number_key.as_deref(),
                currency: candidate_currency.as_deref(),
                total: extracted.total,
                invoice_date: parsed_invoice_date,
            },
            &existing,
        );

        for duplicate in &duplicate_matches {
            let other = existing_rows
                .iter()
                .find(|row| row.id == duplicate.existing_invoice_id);
            let label = duplicate_status_label(duplicate.kind);
            let other_number = other
                .and_then(|row| row.invoice_number.clone())
                .unwrap_or_else(|| duplicate.existing_invoice_id.to_string());
            let other_vendor = other
                .and_then(|row| row.vendor_name.clone())
                .unwrap_or_else(|| "unknown vendor".into());
            issues.push(ValidationIssue::new(
                "DUPLICATE_CANDIDATE",
                Severity::Warning,
                None,
                format!(
                    "{label}: invoice {other_number} from {other_vendor}. {}. Resolve the candidate before approval.",
                    duplicate.reasons.join("; ")
                ),
            ));
        }

        let has_errors = issues.iter().any(|issue| issue.severity == Severity::Error);
        let has_warnings = issues
            .iter()
            .any(|issue| issue.severity == Severity::Warning);
        let missing_required_fields = issues.iter().any(|issue| issue.code.ends_with("_MISSING"));

        // Deterministic automation. It may add labels, assign a reviewer, or
        // hold an invoice for review, but it can never approve or export.
        let rules = operations::enabled_automation_rules(&self.db, organization_id)
            .await
            .map_err(|e| sql_failure("load automation rules", e))?;
        let automation_context = AutomationContext {
            document_type: extracted.document_kind.as_str().to_string(),
            vendor_name: extracted.vendor.name.clone(),
            currency: extracted.normalized_currency(),
            total: extracted.total,
            extraction_quality: extracted.extraction_quality.or(extracted_text.quality),
            validation_status: if has_errors {
                "blocked".into()
            } else if has_warnings {
                "review_required".into()
            } else {
                "valid".into()
            },
            has_duplicates: !duplicate_matches.is_empty(),
            has_blocking_issues: has_errors,
            missing_required_fields,
            country: organization.country.clone(),
        };
        let mut automation_outcome = AutomationOutcome::default();
        let mut matched_rules: Vec<(Uuid, String, Value)> = Vec::new();
        for rule in &rules {
            let spec = match serde_json::from_value::<AutomationSpec>(rule.spec.clone()) {
                Ok(spec) => spec,
                Err(error) => {
                    warn!(rule_id = %rule.id, error = %error, "ignoring invalid automation rule");
                    continue;
                }
            };
            if evaluate_rule(rule.id, &spec, &automation_context, &mut automation_outcome) {
                let actions = serde_json::to_value(&spec.actions).unwrap_or(Value::Null);
                matched_rules.push((rule.id, rule.name.clone(), actions));
            }
        }
        if let Some(reviewer) = automation_outcome.assigned_reviewer {
            match auth::is_organization_member(&self.db, organization_id, reviewer).await {
                Ok(true) => {}
                Ok(false) => {
                    automation_outcome.assigned_reviewer = None;
                    issues.push(ValidationIssue::new(
                        "AUTOMATION_REVIEWER_IGNORED",
                        Severity::Info,
                        None,
                        "An automation rule assigned a reviewer who is not a member of this organization; the assignment was ignored.",
                    ));
                }
                Err(error) => {
                    error!(error = %error, "could not verify automation reviewer membership");
                    automation_outcome.assigned_reviewer = None;
                }
            }
        }

        let labels = automation_outcome
            .labels
            .iter()
            .cloned()
            .collect::<Vec<String>>();
        let provenance = build_provenance(extracted, &issues, &result.provider, &result.model);
        let line_items = extracted
            .line_items
            .iter()
            .map(|item| invoxa_database::invoices::NewLineItem {
                description: item.description.clone(),
                sku: item.sku.clone(),
                quantity: item.quantity,
                unit_of_measure: item.unit_of_measure.clone(),
                unit_price: item.unit_price,
                discount: item.discount,
                tax_rate: item.tax_rate,
                tax_amount: item.tax_amount,
                line_total: item.line_total,
                source_page: item.source_page,
                raw_text: item.raw_text.clone(),
            })
            .collect();
        let tax_lines = extracted
            .tax_lines
            .iter()
            .map(|tax| invoxa_database::invoices::NewTaxLine {
                tax_type: tax.tax_type.clone(),
                rate: tax.rate,
                taxable_amount: tax.taxable_amount,
                amount: tax.amount,
                source_page: tax.source_page,
            })
            .collect();

        let invoice_date_text = extracted
            .original_values
            .get("invoice_date")
            .cloned()
            .or_else(|| extracted.invoice_date.clone());
        let due_date_text = extracted
            .original_values
            .get("due_date")
            .cloned()
            .or_else(|| extracted.due_date.clone());
        let due_date = extracted
            .due_date
            .as_deref()
            .and_then(|value| parse_invoice_date(value).ok())
            .map(|parsed| parsed.date);
        let quality = extracted
            .extraction_quality
            .or(extracted_text.quality)
            .and_then(Decimal::from_f64)
            .filter(|value| *value >= Decimal::ZERO && *value <= Decimal::ONE);
        let vendor = vendors::find_or_create_vendor(
            &self.db,
            organization_id,
            extracted.vendor.name.as_deref(),
            extracted.vendor.tax_id.as_deref(),
        )
        .await
        .map_err(|e| db_failure("link vendor", e))?;

        let new_invoice = NewInvoice {
            organization_id,
            document_id,
            vendor_id: vendor.as_ref().map(|row| row.id),
            document_type: extracted.document_kind.as_str().to_string(),
            invoice_number: trimmed(extracted.invoice_number.as_deref()),
            invoice_number_normalized: extracted.invoice_number_key(),
            invoice_date: parsed_invoice_date,
            due_date,
            invoice_date_text,
            due_date_text,
            purchase_order: extracted.purchase_order.clone(),
            currency: extracted.normalized_currency(),
            payment_terms: extracted.payment_terms.clone(),
            subtotal: extracted.subtotal,
            discount: extracted.discount,
            shipping: extracted.shipping,
            tax_total: extracted.tax_total,
            total: extracted.total,
            amount_paid: extracted.amount_paid,
            balance_due: extracted.balance_due,
            is_credit_note: extracted.is_credit_note,
            status: "review_required".into(),
            extraction_provider: Some(result.provider.clone()),
            extraction_model: Some(result.model.clone()),
            extraction_method: Some(extracted_text.method.as_str().to_string()),
            extraction_quality: quality,
            raw_extraction: Some(result.raw.clone()),
            labels,
            line_items,
            tax_lines,
            provenance,
        };

        let invoice = invoices::create_invoice(&self.db, &new_invoice)
            .await
            .map_err(|e| db_failure("persist extracted invoice", e))?;

        let duplicate_candidates = duplicate_matches
            .iter()
            .map(|matched| {
                let other = existing_rows
                    .iter()
                    .find(|row| row.id == matched.existing_invoice_id);
                (
                    matched.existing_invoice_id,
                    matched.kind.as_str().to_string(),
                    matched.score,
                    json!({
                        "reasons": matched.reasons,
                        "score": matched.score,
                        "kind": matched.kind.as_str(),
                        "other_invoice_number": other.and_then(|row| row.invoice_number.clone()),
                        "other_vendor_name": other.and_then(|row| row.vendor_name.clone()),
                    }),
                )
            })
            .collect::<Vec<_>>();
        invoices::upsert_duplicate_candidates(
            &self.db,
            organization_id,
            invoice.id,
            &duplicate_candidates,
        )
        .await
        .map_err(|e| db_failure("store duplicate candidates", e))?;

        let issues_for_storage = issues
            .iter()
            .map(|issue| {
                (
                    issue.code.clone(),
                    issue.severity.as_str().to_string(),
                    issue.field.clone(),
                    issue.message.clone(),
                )
            })
            .collect::<Vec<_>>();
        invoices::replace_validation_issues(
            &self.db,
            organization_id,
            invoice.id,
            document_id,
            &issues_for_storage,
        )
        .await
        .map_err(|e| db_failure("store validation issues", e))?;

        if let Err(error) = invoices::apply_invoice_automation(
            &self.db,
            organization_id,
            invoice.id,
            &automation_outcome
                .labels
                .iter()
                .cloned()
                .collect::<Vec<_>>(),
            automation_outcome.assigned_reviewer,
        )
        .await
        {
            error!(invoice_id = %invoice.id, error = %error, "could not apply automation assignment");
        }

        for (rule_id, rule_name, actions) in &matched_rules {
            let run = json!({
                "rule_name": rule_name,
                "actions": actions,
                "validation_status": automation_context.validation_status,
                "has_duplicates": !duplicate_matches.is_empty(),
                "has_blocking_issues": has_errors,
                "is_mock": is_mock,
            });
            if let Err(error) = operations::insert_automation_run(
                &self.db,
                organization_id,
                *rule_id,
                invoice.id,
                &run,
            )
            .await
            {
                error!(rule_id = %rule_id, invoice_id = %invoice.id, error = %error, "could not record automation run");
            }
        }

        let ready_for_approval = !has_errors
            && !has_warnings
            && duplicate_matches.is_empty()
            && !is_mock
            && !automation_outcome.hold_for_review;
        let mut final_status = "review_required".to_string();
        if ready_for_approval {
            match invoices::set_invoice_triage_status(
                &self.db,
                organization_id,
                invoice.id,
                "ready_for_approval",
            )
            .await
            {
                Ok(row) => final_status = row.status,
                Err(DbError::Validation(_)) | Err(DbError::InvalidTransition(_)) => {
                    warn!(invoice_id = %invoice.id, "invoice kept in review because triage conditions changed");
                }
                Err(error) => return Err(db_failure("triage invoice", error)),
            }
        }

        let document_next = if final_status == "ready_for_approval" {
            DocumentStatus::ReadyForApproval
        } else {
            DocumentStatus::ReviewRequired
        };
        // Validation is its own lifecycle stage. Entering it explicitly keeps
        // the state machine honest and lets the UI show the final validation
        // stage rather than jumping straight from field extraction.
        documents::transition_document(
            &self.db,
            organization_id,
            document_id,
            DocumentStatus::Validating,
            "validation",
            Some("Validating extracted fields, totals, taxes and duplicate candidates."),
            None,
        )
        .await
        .map_err(|e| db_failure("start document validation", e))?;
        let total_duration = total_started.elapsed().as_millis() as i64;
        documents::transition_document(
            &self.db,
            organization_id,
            document_id,
            document_next,
            "validation",
            Some(&format!(
                "Extraction and validation finished with {} error(s), {} warning(s), and {} duplicate candidate(s).",
                issues.iter().filter(|i| i.severity == Severity::Error).count(),
                issues.iter().filter(|i| i.severity == Severity::Warning).count(),
                duplicate_matches.len()
            )),
            Some(total_duration),
        )
        .await
        .map_err(|e| db_failure("finish document validation", e))?;

        let _ = documents::set_preflight_metadata(
            &self.db,
            organization_id,
            document_id,
            preflight.page_count as i32,
            extracted.document_kind.as_str(),
        )
        .await;

        // Page and AI usage are metered with exact decimal quantities.
        let page_quantity = Decimal::from(preflight.page_count as i64);
        let usage_metadata = json!({
            "text_method": extracted_text.method.as_str(),
            "ocr_pages": extracted_text.ocr_pages,
            "used_ocr": extracted_text.ocr_pages > 0,
            "is_mock": is_mock,
            "provider_warnings": result.warnings.len(),
        });
        if let Err(error) = operations::record_usage(
            &self.db,
            &operations::UsageInput {
                organization_id,
                document_id: Some(document_id),
                kind: "page_processed",
                quantity: page_quantity,
                provider: None,
                model: None,
                input_tokens: None,
                output_tokens: None,
                duration_ms: Some(total_duration),
                estimated_cost_usd: None,
                metadata: usage_metadata.clone(),
            },
        )
        .await
        {
            error!(document_id = %document_id, error = %error, "could not record page usage");
        }
        if extracted_text.ocr_pages > 0 {
            if let Err(error) = operations::record_usage(
                &self.db,
                &operations::UsageInput {
                    organization_id,
                    document_id: Some(document_id),
                    kind: "ocr_page",
                    quantity: Decimal::from(extracted_text.ocr_pages as i64),
                    provider: None,
                    model: None,
                    input_tokens: None,
                    output_tokens: None,
                    duration_ms: Some(text_duration),
                    estimated_cost_usd: None,
                    metadata: usage_metadata.clone(),
                },
            )
            .await
            {
                error!(document_id = %document_id, error = %error, "could not record OCR usage");
            }
        }
        if let Err(error) = operations::record_usage(
            &self.db,
            &operations::UsageInput {
                organization_id,
                document_id: Some(document_id),
                kind: "ai_extraction",
                quantity: Decimal::ONE,
                provider: Some(result.provider.as_str()),
                model: Some(result.model.as_str()),
                input_tokens: result.input_tokens,
                output_tokens: result.output_tokens,
                duration_ms: Some(extraction_duration),
                estimated_cost_usd: None,
                metadata: usage_metadata,
            },
        )
        .await
        {
            error!(document_id = %document_id, error = %error, "could not record AI usage");
        }

        // The outbox enqueue happens before the job is marked successful and
        // uses deterministic event ids, so a retry cannot duplicate them.
        if let Err(error) = crate::events::emit_document_event(
            &self.db,
            organization_id,
            crate::events::EVENT_DOCUMENT_PROCESSING_COMPLETED,
            document_id,
            job.attempts,
            Some(json!({
                "invoice_id": invoice.id,
                "review_status": final_status,
                "is_mock": is_mock,
                "total": extracted.total.map(|value| value.to_string()),
                "currency": extracted.normalized_currency(),
            })),
        )
        .await
        {
            return Err(sql_failure(
                "queue document.processing_completed webhook",
                error,
            ));
        }
        if let Err(error) = crate::events::emit_invoice_event(
            &self.db,
            organization_id,
            crate::events::EVENT_INVOICE_PROCESSED,
            invoice.id,
            Some(json!({
                "text_method": extracted_text.method.as_str(),
                "processing_duration_ms": total_duration,
            })),
        )
        .await
        {
            return Err(sql_failure("queue invoice.processed webhook", error));
        }
        if automation_outcome.webhook_requested {
            if let Err(error) = crate::events::emit_invoice_event(
                &self.db,
                organization_id,
                crate::events::EVENT_AUTOMATION_TRIGGERED,
                invoice.id,
                Some(json!({
                    "automation": {
                        "matched_rule_ids": matched_rules.iter().map(|(id, _, _)| *id).collect::<Vec<_>>(),
                        "actions": {
                            "labels": automation_outcome.labels,
                            "assigned_reviewer": automation_outcome.assigned_reviewer,
                            "hold_for_review": automation_outcome.hold_for_review,
                            "queue_export_requested": automation_outcome.export_eligible,
                        }
                    }
                })),
            )
            .await
            {
                return Err(sql_failure("queue automation.triggered webhook", error));
            }
        }

        Ok(PipelineSuccess {
            invoice_id: invoice.id,
            review_status: final_status,
        })
    }

    async fn event(
        &self,
        organization_id: Uuid,
        document_id: Uuid,
        stage: &str,
        status: &str,
        message: Option<String>,
        duration_ms: Option<i64>,
    ) {
        if let Err(error) = documents::insert_document_event(
            &self.db,
            organization_id,
            document_id,
            stage,
            status,
            message.as_deref(),
            duration_ms,
        )
        .await
        {
            warn!(document_id = %document_id, error = %error, "could not record document event");
        }
    }
}

fn retry_delay_seconds(attempts: i32) -> i64 {
    let base = match attempts {
        0 | 1 => 30,
        2 => 60,
        3 => 120,
        4 => 300,
        5 => 600,
        6 => 1_800,
        _ => 3_600,
    };
    let jitter_max = (base / 5).max(1);
    base + rand::thread_rng().gen_range(0..=jitter_max)
}

fn trimmed(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn storage_failure(error: StorageError) -> PipelineFailure {
    warn!(error = %error, "object storage read failed");
    match error {
        StorageError::NotFound => PipelineFailure::terminal(
            "DOCUMENT_OBJECT_MISSING",
            "The stored document file could not be found.",
        ),
        StorageError::InvalidKey => PipelineFailure::terminal(
            "STORAGE_KEY_INVALID",
            "The document storage key is invalid.",
        ),
        StorageError::Io(_) | StorageError::Unavailable(_) => PipelineFailure::retryable(
            "STORAGE_UNAVAILABLE",
            "The document storage backend is temporarily unavailable.",
        ),
    }
}

fn upload_failure(error: ValidationError) -> PipelineFailure {
    let (code, retryable) = match error {
        ValidationError::Io => ("DOCUMENT_IO_ERROR", true),
        ValidationError::Empty => ("DOCUMENT_EMPTY", false),
        ValidationError::TooLarge => ("FILE_TOO_LARGE", false),
        ValidationError::UnsupportedFormat => ("UNSUPPORTED_FORMAT", false),
        ValidationError::EncryptedPdf => ("PDF_ENCRYPTED", false),
        ValidationError::CorruptPdf => ("PDF_CORRUPT", false),
        ValidationError::CorruptImage => ("IMAGE_CORRUPT", false),
        ValidationError::TooManyPages => ("PAGE_LIMIT_EXCEEDED", false),
        ValidationError::ImageTooLarge => ("IMAGE_TOO_LARGE", false),
        ValidationError::InvalidDimensions => ("INVALID_IMAGE_DIMENSIONS", false),
    };
    if retryable {
        PipelineFailure::retryable(code, error.to_string())
    } else {
        PipelineFailure::terminal(code, error.to_string())
    }
}

fn text_failure(error: TextError) -> PipelineFailure {
    if error.retryable() {
        PipelineFailure::retryable(error.code(), error.to_string())
    } else {
        PipelineFailure::terminal(error.code(), error.to_string())
    }
}

fn extraction_failure(error: ExtractionError) -> PipelineFailure {
    if error.retryable() {
        PipelineFailure::retryable(error.code(), error.to_string())
    } else {
        PipelineFailure::terminal(error.code(), error.to_string())
    }
}

fn sql_failure(context: &'static str, error: sqlx::Error) -> PipelineFailure {
    error!(context, error = %error, "workflow database operation failed");
    PipelineFailure::retryable(
        "PROCESSING_DB_ERROR",
        "A database error interrupted processing. The job will be retried.",
    )
}

fn db_failure(context: &'static str, error: DbError) -> PipelineFailure {
    match error {
        DbError::Sqlx(error) => sql_failure(context, error),
        DbError::NotFound => {
            error!(context, "workflow row was not found");
            PipelineFailure::retryable(
                "PROCESSING_STATE_MISSING",
                "A required processing record was not found. The job will be retried.",
            )
        }
        DbError::Conflict(_) | DbError::InvalidTransition(_) | DbError::Validation(_) => {
            error!(context, error = %error, "workflow rejected a state change");
            PipelineFailure::terminal(
                "PROCESSING_STATE_INVALID",
                "The document state changed in a way that prevents this processing attempt.",
            )
        }
    }
}

fn validation_policy(organization: &invoxa_database::auth::OrganizationRow) -> ValidationPolicy {
    let mut policy = ValidationPolicy {
        country: organization.country.clone(),
        ..Default::default()
    };
    let Some(settings) = organization.settings.get("validation") else {
        return policy;
    };
    if let Some(value) = settings
        .get("require_invoice_date")
        .and_then(Value::as_bool)
    {
        policy.require_invoice_date = value;
    }
    if let Some(value) = settings.get("require_po_number").and_then(Value::as_bool) {
        policy.require_po_number = value;
    }
    if let Some(value) = settings
        .get("require_vendor_tax_id")
        .and_then(Value::as_bool)
    {
        policy.require_vendor_tax_id = value;
    }
    if let Some(value) = settings
        .get("require_gstin_for_india")
        .and_then(Value::as_bool)
    {
        policy.require_gstin_for_india = value;
    }
    if let Some(value) = settings.get("amount_tolerance") {
        let raw = value
            .as_str()
            .map(str::to_string)
            .or_else(|| value.as_number().map(ToString::to_string));
        if let Some(raw) = raw {
            if let Ok(tolerance) = Decimal::from_str(&raw) {
                if tolerance >= Decimal::ZERO && tolerance <= Decimal::ONE {
                    policy.amount_tolerance = tolerance;
                }
            }
        }
    }
    if let Some(value) = settings.get("max_due_date_days").and_then(Value::as_i64) {
        if (0..=3650).contains(&value) {
            policy.max_due_date_days = value;
        }
    }
    policy
}

fn minimum_ocr_quality(settings: &Value) -> Option<Decimal> {
    let value = settings.get("extraction")?.get("min_ocr_quality")?;
    let raw = value
        .as_str()
        .map(str::to_string)
        .or_else(|| value.as_number().map(ToString::to_string))?;
    let parsed = Decimal::from_str(&raw).ok()?;
    if parsed >= Decimal::ZERO && parsed <= Decimal::ONE {
        Some(parsed)
    } else {
        None
    }
}

fn field_validation_status(field: &str, issues: &[ValidationIssue]) -> String {
    let mut status = "valid";
    for issue in issues {
        let Some(issue_field) = issue.field.as_deref() else {
            continue;
        };
        let relevant = issue_field == field
            || issue_field.starts_with(&format!("{field}."))
            || issue_field.starts_with(&format!("{field}["));
        if !relevant {
            continue;
        }
        match issue.severity {
            Severity::Error => return "error".into(),
            Severity::Warning => status = "warning",
            Severity::Info => {
                if status == "valid" {
                    status = "info";
                }
            }
        }
    }
    status.into()
}

fn build_provenance(
    invoice: &ExtractedInvoice,
    issues: &[ValidationIssue],
    provider: &str,
    model: &str,
) -> Vec<invoxa_database::invoices::NewProvenance> {
    let mut entries = Vec::new();
    let method = format!("{provider}/{model}");

    let mut push = |field: &str, normalized: Option<String>| {
        let extracted_value = invoice
            .original_values
            .get(field)
            .cloned()
            .or_else(|| normalized.clone());
        if extracted_value.is_none() && normalized.is_none() {
            return;
        }
        let source = invoice.field_sources.get(field);
        let confidence = invoice
            .field_confidence
            .get(field)
            .and_then(|value| Decimal::from_f64(*value))
            .filter(|value| *value >= Decimal::ZERO && *value <= Decimal::ONE);
        entries.push(invoxa_database::invoices::NewProvenance {
            field_path: field.to_string(),
            extracted_value,
            normalized_value: normalized,
            confidence,
            extraction_method: Some(method.clone()),
            source_page: source.and_then(|source| source.page),
            source_text: source.and_then(|source| source.text.clone()),
            bbox: source
                .and_then(|source| source.bbox)
                .map(|bbox| json!([bbox[0], bbox[1], bbox[2], bbox[3]])),
            validation_status: field_validation_status(field, issues),
            human_edited: false,
            previous_value: None,
            edited_by: None,
        });
    };

    push(
        "document_kind",
        Some(invoice.document_kind.as_str().to_string()),
    );
    push("vendor.name", invoice.vendor.name.clone());
    push("vendor.tax_id", invoice.vendor.tax_id.clone());
    push("invoice_number", invoice.invoice_number.clone());
    push(
        "invoice_date",
        invoice
            .invoice_date
            .as_deref()
            .and_then(|value| parse_invoice_date(value).ok())
            .map(|parsed| parsed.date.to_string()),
    );
    push(
        "due_date",
        invoice
            .due_date
            .as_deref()
            .and_then(|value| parse_invoice_date(value).ok())
            .map(|parsed| parsed.date.to_string()),
    );
    push("purchase_order", invoice.purchase_order.clone());
    push("currency", invoice.normalized_currency());
    push("payment_terms", invoice.payment_terms.clone());
    push("subtotal", invoice.subtotal.map(|value| value.to_string()));
    push("discount", invoice.discount.map(|value| value.to_string()));
    push("shipping", invoice.shipping.map(|value| value.to_string()));
    push(
        "tax_total",
        invoice.tax_total.map(|value| value.to_string()),
    );
    push("total", invoice.total.map(|value| value.to_string()));
    push(
        "amount_paid",
        invoice.amount_paid.map(|value| value.to_string()),
    );
    push(
        "balance_due",
        invoice.balance_due.map(|value| value.to_string()),
    );

    entries
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use invoxa_domain::invoice::{DocumentKind, ExtractedInvoice, FieldSource};
    use serde_json::json;

    fn organization(settings: Value) -> invoxa_database::auth::OrganizationRow {
        invoxa_database::auth::OrganizationRow {
            id: Uuid::new_v4(),
            name: "Example".into(),
            slug: "example".into(),
            country: "IN".into(),
            base_currency: "INR".into(),
            settings,
            is_demo: false,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn retry_backoff_grows_and_is_bounded() {
        let first = retry_delay_seconds(1);
        let later = retry_delay_seconds(5);
        assert!((30..=36).contains(&first));
        assert!(later >= 600);
        assert!(retry_delay_seconds(20) <= 4_320);
    }

    #[test]
    fn validation_policy_reads_exact_decimal_settings() {
        let policy = validation_policy(&organization(json!({
            "validation": {
                "amount_tolerance": "0.005",
                "max_due_date_days": 90,
                "require_po_number": true
            }
        })));
        assert_eq!(policy.amount_tolerance, Decimal::new(5, 3));
        assert_eq!(policy.max_due_date_days, 90);
        assert!(policy.require_po_number);
        assert_eq!(policy.country, "IN");
    }

    #[test]
    fn invalid_tolerance_setting_is_ignored() {
        let policy = validation_policy(&organization(json!({
            "validation": { "amount_tolerance": "-1" }
        })));
        assert_eq!(policy.amount_tolerance, Decimal::new(2, 2));
    }

    #[test]
    fn provenance_preserves_zero_and_skips_missing_fields() {
        let invoice = ExtractedInvoice {
            document_kind: DocumentKind::SupplierInvoice,
            vendor: invoxa_domain::invoice::VendorInfo {
                name: Some("Acme".into()),
                ..Default::default()
            },
            invoice_number: Some("INV-1".into()),
            total: Some(Decimal::ZERO),
            subtotal: None,
            field_sources: [(
                "total".to_string(),
                FieldSource {
                    page: Some(1),
                    text: Some("Total 0.00".into()),
                    bbox: None,
                },
            )]
            .into_iter()
            .collect(),
            ..Default::default()
        };
        let entries = build_provenance(&invoice, &[], "mock-heuristic", "rules-v1");
        let total = entries
            .iter()
            .find(|entry| entry.field_path == "total")
            .expect("zero-valued total has provenance");
        assert_eq!(total.normalized_value.as_deref(), Some("0"));
        assert!(entries.iter().all(|entry| entry.field_path != "subtotal"));
    }

    #[test]
    fn field_status_escalates_to_error() {
        let issues = vec![
            ValidationIssue::new("X", Severity::Warning, Some("total"), "warn"),
            ValidationIssue::new("Y", Severity::Error, Some("total"), "error"),
        ];
        assert_eq!(field_validation_status("total", &issues), "error");
        assert_eq!(field_validation_status("currency", &issues), "valid");
    }

    #[test]
    fn ocr_quality_threshold_is_opt_in() {
        assert!(minimum_ocr_quality(&json!({})).is_none());
        assert_eq!(
            minimum_ocr_quality(&json!({"extraction": {"min_ocr_quality": "0.55"}})),
            Some(Decimal::new(55, 2))
        );
        assert!(minimum_ocr_quality(&json!({"extraction": {"min_ocr_quality": "2"}})).is_none());
    }
}
