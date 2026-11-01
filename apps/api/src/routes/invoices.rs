use crate::auth::{audit, AuthContext};
use crate::error::{ApiError, ApiResult};
use crate::presenters::{
    approval_event_json, duplicate_json, invoice_json, invoice_list_json, issue_json,
    line_item_json, provenance_json, review_json,
};
use crate::state::AppState;
use crate::util::{
    decimal_value, limit_offset, optional_text, parse_iso_date, required_text, ApiJson, ApiPath,
    ApiQuery,
};
use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use chrono::NaiveDate;
use invoxa_database::{documents, invoices as db_invoices, vendors};
use invoxa_domain::invoice::{DocumentKind, ExtractedInvoice, LineItem, TaxLine, VendorInfo};
use invoxa_domain::normalization::{
    is_known_currency, normalize_currency, normalize_invoice_number, parse_invoice_date,
};
use invoxa_domain::role::Permission;
use invoxa_domain::status::{DocumentStatus, InvoiceStatus};
use invoxa_domain::validation::{
    validate_invoice, Severity, ValidationContext, ValidationIssue, ValidationPolicy,
};
use rust_decimal::Decimal;
use serde::Deserialize;
use serde_json::{json, Map, Value};
use uuid::Uuid;

#[derive(Deserialize)]
pub struct InvoiceFilters {
    pub status: Option<String>,
    pub search: Option<String>,
    pub vendor_id: Option<Uuid>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub currency: Option<String>,
    pub assigned_to_me: Option<bool>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Deserialize)]
pub struct DecisionInput {
    pub version: i32,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub acknowledge_mock: Option<bool>,
}

#[derive(Deserialize)]
pub struct DuplicateDecisionInput {
    pub status: String,
    #[serde(default)]
    pub note: Option<String>,
}

#[derive(Deserialize)]
pub struct NoteInput {
    pub notes: String,
}

/// Distinguishes an omitted field from an explicit JSON null. `Option<Value>`
/// alone cannot do this because serde maps both to `None`.
#[derive(Debug, Default)]
pub struct PatchField(Option<Value>);

impl<'de> Deserialize<'de> for PatchField {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Ok(Self(Some(Value::deserialize(deserializer)?)))
    }
}

impl PatchField {
    fn is_set(&self) -> bool {
        self.0.is_some()
    }

    fn value(&self) -> Option<&Value> {
        self.0.as_ref()
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InvoicePatch {
    pub version: Option<i32>,
    #[serde(default)]
    pub vendor_id: PatchField,
    #[serde(default)]
    pub vendor_name: PatchField,
    #[serde(default)]
    pub document_type: PatchField,
    #[serde(default)]
    pub invoice_number: PatchField,
    #[serde(default)]
    pub invoice_date: PatchField,
    #[serde(default)]
    pub due_date: PatchField,
    #[serde(default)]
    pub purchase_order: PatchField,
    #[serde(default)]
    pub currency: PatchField,
    #[serde(default)]
    pub payment_terms: PatchField,
    #[serde(default)]
    pub subtotal: PatchField,
    #[serde(default)]
    pub discount: PatchField,
    #[serde(default)]
    pub shipping: PatchField,
    #[serde(default)]
    pub tax_total: PatchField,
    #[serde(default)]
    pub total: PatchField,
    #[serde(default)]
    pub amount_paid: PatchField,
    #[serde(default)]
    pub balance_due: PatchField,
    #[serde(default)]
    pub is_credit_note: PatchField,
    #[serde(default)]
    pub labels: PatchField,
    #[serde(default)]
    pub line_items: PatchField,
    #[serde(default)]
    pub tax_lines: PatchField,
    #[serde(default)]
    pub notes: PatchField,
}

impl InvoicePatch {
    fn submitted_fields(&self) -> Vec<&'static str> {
        let mut fields = Vec::new();
        macro_rules! add {
            ($field:ident, $name:literal) => {
                if self.$field.is_set() {
                    fields.push($name);
                }
            };
        }
        add!(vendor_id, "vendor_id");
        add!(vendor_name, "vendor_name");
        add!(document_type, "document_type");
        add!(invoice_number, "invoice_number");
        add!(invoice_date, "invoice_date");
        add!(due_date, "due_date");
        add!(purchase_order, "purchase_order");
        add!(currency, "currency");
        add!(payment_terms, "payment_terms");
        add!(subtotal, "subtotal");
        add!(discount, "discount");
        add!(shipping, "shipping");
        add!(tax_total, "tax_total");
        add!(total, "total");
        add!(amount_paid, "amount_paid");
        add!(balance_due, "balance_due");
        add!(is_credit_note, "is_credit_note");
        add!(labels, "labels");
        add!(line_items, "line_items");
        add!(tax_lines, "tax_lines");
        add!(notes, "notes");
        fields
    }
}

pub async fn list(
    State(state): State<AppState>,
    auth: AuthContext,
    ApiQuery(filters): ApiQuery<InvoiceFilters>,
) -> ApiResult<Json<Value>> {
    auth.require_scope(Permission::ViewDocuments, "invoices:read")?;
    let (limit, offset) = limit_offset(filters.limit, filters.offset, 25);
    let search = filters
        .search
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if search.map(str::len).unwrap_or(0) > 200 {
        return Err(ApiError::validation(
            "search must be at most 200 characters",
        ));
    }
    if let Some(status) = filters.status.as_deref().filter(|s| !s.trim().is_empty()) {
        InvoiceStatus::parse(status)
            .ok_or_else(|| ApiError::validation("Unknown invoice status filter."))?;
    }
    let from = filters
        .from
        .as_deref()
        .filter(|v| !v.trim().is_empty())
        .map(|value| parse_iso_date(value, "from"))
        .transpose()?;
    let to = filters
        .to
        .as_deref()
        .filter(|v| !v.trim().is_empty())
        .map(|value| parse_iso_date(value, "to"))
        .transpose()?;
    let currency = filters
        .currency
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| {
            normalize_currency(value)
                .filter(|code| is_known_currency(code))
                .ok_or_else(|| ApiError::validation("currency must be a supported ISO code"))
        })
        .transpose()?;
    let assigned_to = if filters.assigned_to_me.unwrap_or(false) {
        Some(auth.user_id.or(auth.key_created_by).unwrap_or(Uuid::nil()))
    } else {
        None
    };
    let (rows, total) = db_invoices::list_invoices(
        &state.db,
        auth.organization_id,
        filters
            .status
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty()),
        search,
        filters.vendor_id,
        from,
        to,
        currency.as_deref(),
        assigned_to,
        limit,
        offset,
    )
    .await?;
    Ok(Json(json!({
        "items": rows.iter().map(invoice_list_json).collect::<Vec<_>>(),
        "total": total,
        "limit": limit,
        "offset": offset,
    })))
}

pub async fn detail(
    State(state): State<AppState>,
    auth: AuthContext,
    ApiPath(invoice_id): ApiPath<Uuid>,
) -> ApiResult<Json<Value>> {
    auth.require_scope(Permission::ViewDocuments, "invoices:read")?;
    let invoice = load_invoice(&state, &auth, invoice_id).await?;
    let payload = invoice_detail_payload(&state, &auth, &invoice).await?;
    Ok(Json(payload))
}

async fn load_invoice(
    state: &AppState,
    auth: &AuthContext,
    invoice_id: Uuid,
) -> ApiResult<db_invoices::InvoiceRow> {
    db_invoices::get_invoice(&state.db, auth.organization_id, invoice_id)
        .await?
        .ok_or_else(|| ApiError::not_found("Invoice not found."))
}

async fn invoice_detail_payload(
    state: &AppState,
    auth: &AuthContext,
    invoice: &db_invoices::InvoiceRow,
) -> ApiResult<Value> {
    let lines = db_invoices::line_items(&state.db, auth.organization_id, invoice.id).await?;
    let taxes = db_invoices::tax_lines(&state.db, auth.organization_id, invoice.id).await?;
    let provenance = db_invoices::provenance(&state.db, auth.organization_id, invoice.id).await?;
    let issues =
        db_invoices::validation_issues(&state.db, auth.organization_id, invoice.id).await?;
    let duplicates = db_invoices::duplicates(&state.db, auth.organization_id, invoice.id).await?;
    let approvals =
        db_invoices::approval_events(&state.db, auth.organization_id, invoice.id).await?;
    let reviews = db_invoices::reviews(&state.db, auth.organization_id, invoice.id).await?;
    let document =
        documents::get_document(&state.db, auth.organization_id, invoice.document_id).await?;

    let unresolved_errors = issues
        .iter()
        .filter(|issue| issue.resolved_at.is_none() && issue.severity == "error")
        .count();
    let unresolved_warnings = issues
        .iter()
        .filter(|issue| issue.resolved_at.is_none() && issue.severity == "warning")
        .count();
    let open_duplicates = duplicates
        .iter()
        .filter(|duplicate| duplicate.status == "open")
        .count();
    let confirmed_duplicates = duplicates
        .iter()
        .filter(|duplicate| duplicate.status == "confirmed")
        .count();
    let editable = matches!(
        invoice.status.as_str(),
        "review_required" | "ready_for_approval" | "rejected"
    );
    let can_approve = matches!(
        invoice.status.as_str(),
        "review_required" | "ready_for_approval"
    ) && auth.allows(Permission::ApproveInvoice, "invoices:approve");
    let can_reject = matches!(
        invoice.status.as_str(),
        "review_required" | "ready_for_approval"
    ) && auth.allows(Permission::RejectInvoice, "invoices:reject");

    let mut payload = invoice_json(invoice);
    if let Some(object) = payload.as_object_mut() {
        object.insert(
            "line_items".into(),
            Value::Array(lines.iter().map(line_item_json).collect()),
        );
        object.insert(
            "tax_lines".into(),
            Value::Array(taxes.iter().map(crate::presenters::tax_line_json).collect()),
        );
        object.insert(
            "provenance".into(),
            Value::Array(provenance.iter().map(provenance_json).collect()),
        );
        object.insert(
            "issues".into(),
            Value::Array(issues.iter().map(issue_json).collect()),
        );
        object.insert(
            "duplicates".into(),
            Value::Array(duplicates.iter().map(duplicate_json).collect()),
        );
        object.insert(
            "approval_events".into(),
            Value::Array(approvals.iter().map(approval_event_json).collect()),
        );
        object.insert(
            "reviews".into(),
            Value::Array(reviews.iter().map(review_json).collect()),
        );
        object.insert(
            "document".into(),
            document
                .as_ref()
                .map(|doc| {
                    json!({
                        "id": doc.id,
                        "filename": doc.original_filename,
                        "status": doc.processing_status,
                        "content_type": doc.content_type,
                        "content_url": format!("/api/v1/documents/{}/content", doc.id),
                    })
                })
                .unwrap_or(Value::Null),
        );
        object.insert(
            "blocking".into(),
            json!({
                "error_count": unresolved_errors,
                "warning_count": unresolved_warnings,
                "open_duplicates": open_duplicates,
                "confirmed_duplicates": confirmed_duplicates,
                "approval_blocked": unresolved_errors > 0 || open_duplicates > 0 || confirmed_duplicates > 0,
                "mock_requires_acknowledgement": crate::presenters::is_mock_provider(invoice.extraction_provider.as_deref()),
            }),
        );
        object.insert(
            "available_actions".into(),
            json!({
                "edit": editable && auth.allows(Permission::EditInvoice, "invoices:write"),
                "approve": can_approve,
                "reject": can_reject,
                "return_for_correction": matches!(invoice.status.as_str(), "ready_for_approval" | "rejected")
                    && auth.allows(Permission::EditInvoice, "invoices:write"),
                "export": auth.allows(Permission::ExportInvoices, "exports:write"),
                "retry_processing": auth.allows(Permission::UploadDocuments, "documents:write"),
            }),
        );
    }
    Ok(payload)
}

pub async fn update(
    State(state): State<AppState>,
    auth: AuthContext,
    ApiPath(invoice_id): ApiPath<Uuid>,
    ApiJson(patch): ApiJson<InvoicePatch>,
) -> ApiResult<Json<Value>> {
    auth.require_scope(Permission::EditInvoice, "invoices:write")?;
    let expected_version = patch.version.ok_or_else(|| {
        ApiError::validation("version is required so concurrent edits cannot overwrite each other")
    })?;
    if expected_version < 0 {
        return Err(ApiError::validation("version is invalid"));
    }
    let invoice = load_invoice(&state, &auth, invoice_id).await?;
    if invoice.version != expected_version {
        return Err(ApiError::conflict(
            "The invoice was modified by someone else. Reload it and try again.",
        ));
    }
    if matches!(invoice.status.as_str(), "approved" | "exported") {
        return Err(ApiError::conflict(
            "Approved and exported invoices cannot be edited. Reopening is restricted to an owner and is not enabled in this build.",
        ));
    }
    if patch.submitted_fields().is_empty() {
        return Err(ApiError::validation("No editable fields were provided."));
    }
    let actor = auth.actor_id()?;
    let lines = db_invoices::line_items(&state.db, auth.organization_id, invoice_id).await?;
    let taxes = db_invoices::tax_lines(&state.db, auth.organization_id, invoice_id).await?;
    let old_provenance =
        db_invoices::provenance(&state.db, auth.organization_id, invoice_id).await?;
    let existing_issues =
        db_invoices::validation_issues(&state.db, auth.organization_id, invoice_id).await?;

    let (vendor_id, vendor) = resolve_vendor(&state, &auth, invoice.vendor_id, &patch).await?;
    let document_kind = patched_document_kind(&invoice.document_type, &patch.document_type)?;
    let invoice_number = patched_text(
        invoice.invoice_number.clone(),
        &patch.invoice_number,
        "invoice_number",
        120,
    )?;
    let invoice_date = patched_date(
        invoice.invoice_date,
        invoice.invoice_date_text.clone(),
        &patch.invoice_date,
        "invoice_date",
    )?;
    let due_date = patched_date(
        invoice.due_date,
        invoice.due_date_text.clone(),
        &patch.due_date,
        "due_date",
    )?;
    let purchase_order = patched_text(
        invoice.purchase_order.clone(),
        &patch.purchase_order,
        "purchase_order",
        80,
    )?;
    let currency = patched_currency(invoice.currency.clone(), &patch.currency)?;
    let payment_terms = patched_text(
        invoice.payment_terms.clone(),
        &patch.payment_terms,
        "payment_terms",
        200,
    )?;
    let subtotal = patched_decimal(invoice.subtotal, &patch.subtotal, "subtotal")?;
    let discount = patched_decimal(invoice.discount, &patch.discount, "discount")?;
    let shipping = patched_decimal(invoice.shipping, &patch.shipping, "shipping")?;
    let tax_total = patched_decimal(invoice.tax_total, &patch.tax_total, "tax_total")?;
    let total = patched_decimal(invoice.total, &patch.total, "total")?;
    let amount_paid = patched_decimal(invoice.amount_paid, &patch.amount_paid, "amount_paid")?;
    let balance_due = patched_decimal(invoice.balance_due, &patch.balance_due, "balance_due")?;
    let labels = patched_labels(invoice.labels.clone(), &patch.labels)?;
    let credit_patch = patched_bool(
        invoice.is_credit_note,
        &patch.is_credit_note,
        "is_credit_note",
    )?;
    let is_credit_note = if document_kind == DocumentKind::CreditNote {
        true
    } else {
        credit_patch
    };
    let line_items = if patch.line_items.is_set() {
        parse_line_items(&patch.line_items)?
    } else {
        lines.iter().map(line_item_from_row).collect()
    };
    let tax_lines = if patch.tax_lines.is_set() {
        parse_tax_lines(&patch.tax_lines)?
    } else {
        taxes.iter().map(tax_line_from_row).collect()
    };
    let notes = optional_text(patch.notes.value(), "notes", 2_000)?;

    let document_type = document_kind.as_str().to_string();
    let normalized_number = invoice_number
        .as_deref()
        .map(normalize_invoice_number)
        .filter(|value| !value.is_empty());
    let vendor_name = vendor.as_ref().map(|vendor| vendor.name.clone());
    let vendor_tax_id = vendor.as_ref().and_then(|vendor| vendor.tax_id.clone());

    let extracted = ExtractedInvoice {
        document_kind,
        vendor: VendorInfo {
            name: vendor_name.clone(),
            tax_id: vendor_tax_id.clone(),
            address: vendor.as_ref().and_then(|v| v.address.clone()),
            email: vendor.as_ref().and_then(|v| v.email.clone()),
            phone: vendor.as_ref().and_then(|v| v.phone.clone()),
        },
        invoice_number: invoice_number.clone(),
        invoice_date: invoice_date.date.map(|date| date.to_string()),
        due_date: due_date.date.map(|date| date.to_string()),
        purchase_order: purchase_order.clone(),
        currency: currency.clone(),
        payment_terms: payment_terms.clone(),
        subtotal,
        discount,
        shipping,
        tax_total,
        total,
        amount_paid,
        balance_due,
        is_credit_note,
        line_items: line_items
            .iter()
            .map(|item| LineItem {
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
            .collect(),
        tax_lines: tax_lines
            .iter()
            .map(|item| TaxLine {
                tax_type: item.tax_type.clone(),
                rate: item.rate,
                taxable_amount: item.taxable_amount,
                amount: item.amount,
                source_page: item.source_page,
            })
            .collect(),
        original_values: invoice_date
            .text
            .clone()
            .map(|text| [("invoice_date".to_string(), text)].into_iter().collect())
            .unwrap_or_default(),
        field_sources: Default::default(),
        field_confidence: Default::default(),
        extraction_quality: invoice
            .extraction_quality
            .and_then(|quality| quality.to_string().parse::<f64>().ok()),
        warnings: Vec::new(),
    };
    if let Err(errors) = extracted.validate_shape() {
        return Err(ApiError::validation(errors.join("; ")));
    }
    let policy = ValidationPolicy::from_settings(&auth.country, &auth.settings);
    let validation = validate_invoice(
        &extracted,
        &ValidationContext {
            policy,
            extraction_quality: extracted.extraction_quality,
        },
    );

    let mut issue_tuples: Vec<(String, String, Option<String>, String)> = validation
        .issues
        .iter()
        .map(|issue| {
            (
                issue.code.clone(),
                issue.severity.as_str().to_string(),
                issue.field.clone(),
                issue.message.clone(),
            )
        })
        .collect();
    const ADVISORY_CODES: &[&str] = &[
        "EXTRACTION_WARNING",
        "MOCK_EXTRACTION",
        "LOW_OCR_QUALITY",
        "DUPLICATE_CANDIDATE",
        "AUTOMATION_REVIEWER_IGNORED",
    ];
    for issue in &existing_issues {
        if issue.resolved_at.is_some() || !ADVISORY_CODES.contains(&issue.code.as_str()) {
            continue;
        }
        let tuple = (
            issue.code.clone(),
            issue.severity.clone(),
            issue.field_path.clone(),
            issue.message.clone(),
        );
        if !issue_tuples
            .iter()
            .any(|existing| existing.0 == tuple.0 && existing.2 == tuple.2 && existing.3 == tuple.3)
        {
            issue_tuples.push(tuple);
        }
    }

    let provenance = build_provenance(
        &old_provenance,
        &validation.issues,
        &issue_tuples,
        actor,
        &[
            ("document_type", Some(document_type.clone())),
            ("vendor.name", vendor_name.clone()),
            ("vendor.tax_id", vendor_tax_id.clone()),
            ("invoice_number", invoice_number.clone()),
            ("invoice_date", invoice_date.date.map(|d| d.to_string())),
            ("due_date", due_date.date.map(|d| d.to_string())),
            ("purchase_order", purchase_order.clone()),
            ("currency", currency.clone()),
            ("payment_terms", payment_terms.clone()),
            ("subtotal", subtotal.map(|v| v.to_string())),
            ("discount", discount.map(|v| v.to_string())),
            ("shipping", shipping.map(|v| v.to_string())),
            ("tax_total", tax_total.map(|v| v.to_string())),
            ("total", total.map(|v| v.to_string())),
            ("amount_paid", amount_paid.map(|v| v.to_string())),
            ("balance_due", balance_due.map(|v| v.to_string())),
            ("is_credit_note", Some(is_credit_note.to_string())),
        ],
    );

    let new_invoice = db_invoices::NewInvoice {
        organization_id: auth.organization_id,
        document_id: invoice.document_id,
        vendor_id,
        document_type,
        invoice_number,
        invoice_number_normalized: normalized_number,
        invoice_date: invoice_date.date,
        due_date: due_date.date,
        invoice_date_text: invoice_date.text,
        due_date_text: due_date.text,
        purchase_order,
        currency,
        payment_terms,
        subtotal,
        discount,
        shipping,
        tax_total,
        total,
        amount_paid,
        balance_due,
        is_credit_note,
        status: "review_required".into(),
        extraction_provider: invoice.extraction_provider.clone(),
        extraction_model: invoice.extraction_model.clone(),
        extraction_method: invoice.extraction_method.clone(),
        extraction_quality: invoice.extraction_quality,
        raw_extraction: None,
        labels,
        line_items,
        tax_lines,
        provenance,
    };
    let submitted = patch.submitted_fields();
    let changed_fields = json!({
        "submitted_fields": submitted,
        "version_before": expected_version,
        "manual_edit": true,
    });
    let reviewer_action =
        if matches!(patch.line_items.value(), Some(Value::Array(_))) || patch.tax_lines.is_set() {
            "line_item_edit"
        } else {
            "field_edit"
        };
    let updated = db_invoices::update_invoice(
        &state.db,
        auth.organization_id,
        invoice_id,
        expected_version,
        actor,
        &new_invoice,
        "review_required",
        &issue_tuples,
        &changed_fields,
        reviewer_action,
    )
    .await?;

    if let Some(doc) =
        documents::get_document(&state.db, auth.organization_id, invoice.document_id).await?
    {
        if let Some(current) = DocumentStatus::parse(&doc.processing_status) {
            if current != DocumentStatus::ReviewRequired
                && current.can_transition_to(DocumentStatus::ReviewRequired)
            {
                if let Err(error) = documents::transition_document(
                    &state.db,
                    auth.organization_id,
                    doc.id,
                    DocumentStatus::ReviewRequired,
                    "review",
                    Some("Invoice corrected by a reviewer; re-review required"),
                    None,
                )
                .await
                {
                    tracing::warn!(document_id = %doc.id, error = %error, "could not return document to review");
                }
            }
        }
    }
    if let Some(notes) = notes {
        let _ = db_invoices::insert_review_note(
            &state.db,
            auth.organization_id,
            invoice_id,
            actor,
            "note",
            &notes,
        )
        .await;
    }

    let _ = invoxa_workflows::events::emit_invoice_event(
        &state.db,
        auth.organization_id,
        invoxa_workflows::events::EVENT_INVOICE_UPDATED,
        invoice_id,
        Some(json!({"reviewer_action": reviewer_action})),
    )
    .await;
    audit(
        &state,
        &auth,
        "invoice.updated",
        Some("invoice"),
        Some(invoice_id),
        json!({
            "submitted_fields": changed_fields["submitted_fields"],
            "reviewer_action": reviewer_action,
            "new_version": updated.version,
        }),
    )
    .await;

    let payload = invoice_detail_payload(&state, &auth, &updated).await?;
    Ok(Json(payload))
}

pub async fn approve(
    State(state): State<AppState>,
    auth: AuthContext,
    ApiPath(invoice_id): ApiPath<Uuid>,
    ApiJson(input): ApiJson<DecisionInput>,
) -> ApiResult<Json<Value>> {
    auth.require_scope(Permission::ApproveInvoice, "invoices:approve")?;
    let invoice = load_invoice(&state, &auth, invoice_id).await?;
    if invoice.version != input.version {
        return Err(ApiError::conflict(
            "The invoice changed since it was loaded. Reload it before approving.",
        ));
    }
    if !matches!(
        invoice.status.as_str(),
        "review_required" | "ready_for_approval"
    ) {
        return Err(ApiError::conflict(format!(
            "An invoice in status '{}' cannot be approved.",
            invoice.status
        )));
    }
    let blockers =
        db_invoices::unresolved_blocking_issues(&state.db, auth.organization_id, invoice_id)
            .await?;
    if !blockers.is_empty() {
        return Err(ApiError::conflict(
            "Resolve all blocking validation errors before approval.",
        )
        .with_details(json!({
            "issues": blockers.iter().map(|(code, message)| json!({"code": code, "message": message})).collect::<Vec<_>>()
        })));
    }
    let duplicates = db_invoices::duplicates(&state.db, auth.organization_id, invoice_id).await?;
    let open = duplicates.iter().filter(|d| d.status == "open").count();
    if open > 0 {
        return Err(
            ApiError::conflict("Resolve all open duplicate candidates before approval.")
                .with_details(json!({"open_duplicates": open})),
        );
    }
    let confirmed = duplicates
        .iter()
        .filter(|d| d.status == "confirmed")
        .count();
    if confirmed > 0 {
        return Err(ApiError::conflict(
            "This invoice has a confirmed duplicate and cannot be approved. Reject it instead.",
        ));
    }
    if crate::presenters::is_mock_provider(invoice.extraction_provider.as_deref())
        && !input.acknowledge_mock.unwrap_or(false)
    {
        return Err(ApiError::validation(
            "This record contains synthetic data from the deterministic mock provider. Acknowledge the mock extraction explicitly before approving.",
        ));
    }
    let reason = input
        .reason
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if reason.map(str::len).unwrap_or(0) > 500 {
        return Err(ApiError::validation(
            "reason must be at most 500 characters",
        ));
    }
    let actor = auth.actor_id()?;
    let updated = db_invoices::set_invoice_status(
        &state.db,
        auth.organization_id,
        invoice_id,
        input.version,
        "approved",
        actor,
        "approved",
        reason,
    )
    .await?;
    let _ = invoxa_workflows::events::emit_invoice_event(
        &state.db,
        auth.organization_id,
        invoxa_workflows::events::EVENT_INVOICE_APPROVED,
        invoice_id,
        Some(json!({"mock_acknowledged": input.acknowledge_mock.unwrap_or(false)})),
    )
    .await;
    audit(
        &state,
        &auth,
        "invoice.approved",
        Some("invoice"),
        Some(invoice_id),
        json!({
            "reason": reason,
            "previous_status": invoice.status,
            "new_version": updated.version,
        }),
    )
    .await;
    let payload = invoice_detail_payload(&state, &auth, &updated).await?;
    Ok(Json(payload))
}

pub async fn return_for_correction(
    State(state): State<AppState>,
    auth: AuthContext,
    ApiPath(invoice_id): ApiPath<Uuid>,
    ApiJson(input): ApiJson<DecisionInput>,
) -> ApiResult<Json<Value>> {
    auth.require_scope(Permission::EditInvoice, "invoices:write")?;
    let invoice = load_invoice(&state, &auth, invoice_id).await?;
    if invoice.version != input.version {
        return Err(ApiError::conflict(
            "The invoice changed since it was loaded. Reload it before returning it for correction.",
        ));
    }
    if !matches!(invoice.status.as_str(), "ready_for_approval" | "rejected") {
        return Err(ApiError::conflict(format!(
            "An invoice in status '{}' cannot be returned for correction.",
            invoice.status
        )));
    }
    let reason = required_text(
        input.reason.as_deref().unwrap_or_default(),
        "reason",
        3,
        500,
    )?;
    let actor = auth.actor_id()?;
    let updated = db_invoices::set_invoice_status(
        &state.db,
        auth.organization_id,
        invoice_id,
        input.version,
        "review_required",
        actor,
        "returned",
        Some(&reason),
    )
    .await?;
    let _ = invoxa_workflows::events::emit_invoice_event(
        &state.db,
        auth.organization_id,
        invoxa_workflows::events::EVENT_INVOICE_UPDATED,
        invoice_id,
        Some(json!({"action": "returned_for_correction", "reason": reason})),
    )
    .await;
    audit(
        &state,
        &auth,
        "invoice.returned_for_correction",
        Some("invoice"),
        Some(invoice_id),
        json!({"reason": reason, "previous_status": invoice.status, "new_version": updated.version}),
    )
    .await;
    let payload = invoice_detail_payload(&state, &auth, &updated).await?;
    Ok(Json(payload))
}

pub async fn reject(
    State(state): State<AppState>,
    auth: AuthContext,
    ApiPath(invoice_id): ApiPath<Uuid>,
    ApiJson(input): ApiJson<DecisionInput>,
) -> ApiResult<Json<Value>> {
    auth.require_scope(Permission::RejectInvoice, "invoices:reject")?;
    let invoice = load_invoice(&state, &auth, invoice_id).await?;
    if invoice.version != input.version {
        return Err(ApiError::conflict(
            "The invoice changed since it was loaded. Reload it before rejecting.",
        ));
    }
    if !matches!(
        invoice.status.as_str(),
        "review_required" | "ready_for_approval"
    ) {
        return Err(ApiError::conflict(format!(
            "An invoice in status '{}' cannot be rejected.",
            invoice.status
        )));
    }
    let reason = required_text(
        input.reason.as_deref().unwrap_or_default(),
        "reason",
        3,
        500,
    )?;
    let actor = auth.actor_id()?;
    let updated = db_invoices::set_invoice_status(
        &state.db,
        auth.organization_id,
        invoice_id,
        input.version,
        "rejected",
        actor,
        "rejected",
        Some(&reason),
    )
    .await?;
    let _ = invoxa_workflows::events::emit_invoice_event(
        &state.db,
        auth.organization_id,
        invoxa_workflows::events::EVENT_INVOICE_REJECTED,
        invoice_id,
        Some(json!({"reason": reason})),
    )
    .await;
    audit(
        &state,
        &auth,
        "invoice.rejected",
        Some("invoice"),
        Some(invoice_id),
        json!({"reason": reason, "new_version": updated.version}),
    )
    .await;
    let payload = invoice_detail_payload(&state, &auth, &updated).await?;
    Ok(Json(payload))
}

pub async fn resolve_duplicate(
    State(state): State<AppState>,
    auth: AuthContext,
    ApiPath((invoice_id, duplicate_id)): ApiPath<(Uuid, Uuid)>,
    ApiJson(input): ApiJson<DuplicateDecisionInput>,
) -> ApiResult<Json<Value>> {
    auth.require_scope(Permission::EditInvoice, "invoices:write")?;
    if !matches!(input.status.as_str(), "confirmed" | "dismissed" | "open") {
        return Err(ApiError::validation(
            "status must be one of confirmed, dismissed or open",
        ));
    }
    let note_value = input.note.as_ref().map(|note| Value::String(note.clone()));
    let note = optional_text(note_value.as_ref(), "note", 500)?;
    // The mapping above intentionally goes through the shared optional-text
    // validator so control characters and length are checked consistently.
    let invoice = load_invoice(&state, &auth, invoice_id).await?;
    if matches!(invoice.status.as_str(), "approved" | "exported") {
        return Err(ApiError::conflict(
            "Duplicates cannot be changed after approval.",
        ));
    }
    let actor = auth.actor_id()?;
    db_invoices::resolve_duplicate(
        &state.db,
        auth.organization_id,
        invoice_id,
        duplicate_id,
        &input.status,
        note.as_deref(),
        actor,
    )
    .await?;
    audit(
        &state,
        &auth,
        "invoice.duplicate_resolved",
        Some("invoice"),
        Some(invoice_id),
        json!({"duplicate_id": duplicate_id, "status": input.status, "note": note}),
    )
    .await;
    let duplicates = db_invoices::duplicates(&state.db, auth.organization_id, invoice_id).await?;
    Ok(Json(json!({
        "duplicates": duplicates.iter().map(duplicate_json).collect::<Vec<_>>()
    })))
}

pub async fn add_note(
    State(state): State<AppState>,
    auth: AuthContext,
    ApiPath(invoice_id): ApiPath<Uuid>,
    ApiJson(input): ApiJson<NoteInput>,
) -> ApiResult<impl axum::response::IntoResponse> {
    auth.require_scope(Permission::EditInvoice, "invoices:write")?;
    let notes = required_text(&input.notes, "notes", 1, 2_000)?;
    load_invoice(&state, &auth, invoice_id).await?;
    let actor = auth.actor_id()?;
    db_invoices::insert_review_note(
        &state.db,
        auth.organization_id,
        invoice_id,
        actor,
        "note",
        &notes,
    )
    .await?;
    audit(
        &state,
        &auth,
        "invoice.note_added",
        Some("invoice"),
        Some(invoice_id),
        json!({"notes": notes}),
    )
    .await;
    Ok(StatusCode::CREATED)
}

// ---------------------------------------------------------------------------
// Patch helpers
// ---------------------------------------------------------------------------

async fn resolve_vendor(
    state: &AppState,
    auth: &AuthContext,
    current_vendor_id: Option<Uuid>,
    patch: &InvoicePatch,
) -> ApiResult<(Option<Uuid>, Option<vendors::VendorRow>)> {
    if patch.vendor_id.is_set() {
        return match patch.vendor_id.value() {
            Some(Value::Null) => Ok((None, None)),
            Some(Value::String(raw)) => {
                let id = Uuid::parse_str(raw.trim())
                    .map_err(|_| ApiError::validation("vendor_id is invalid"))?;
                let vendor = vendors::get_vendor(&state.db, auth.organization_id, id)
                    .await?
                    .ok_or_else(|| ApiError::not_found("Vendor not found."))?;
                Ok((Some(id), Some(vendor)))
            }
            _ => Err(ApiError::validation("vendor_id must be a UUID or null")),
        };
    }
    if patch.vendor_name.is_set() {
        let name = optional_text(patch.vendor_name.value(), "vendor_name", 300)?;
        let vendor =
            vendors::find_or_create_vendor(&state.db, auth.organization_id, name.as_deref(), None)
                .await?;
        return Ok((vendor.as_ref().map(|row| row.id), vendor));
    }
    let vendor = match current_vendor_id {
        Some(id) => vendors::get_vendor(&state.db, auth.organization_id, id).await?,
        None => None,
    };
    Ok((current_vendor_id.filter(|_| vendor.is_some()), vendor))
}

fn patched_text(
    current: Option<String>,
    patch: &PatchField,
    field: &str,
    max_chars: usize,
) -> ApiResult<Option<String>> {
    if patch.is_set() {
        optional_text(patch.value(), field, max_chars)
    } else {
        Ok(current)
    }
}

fn patched_currency(current: Option<String>, patch: &PatchField) -> ApiResult<Option<String>> {
    if !patch.is_set() {
        return Ok(current);
    }
    match patch.value() {
        Some(Value::Null) => Ok(None),
        Some(Value::String(raw)) if !raw.trim().is_empty() => {
            let code = normalize_currency(raw)
                .filter(|code| is_known_currency(code))
                .ok_or_else(|| ApiError::validation("currency must be a supported ISO code"))?;
            Ok(Some(code))
        }
        Some(Value::String(_)) => Ok(None),
        _ => Err(ApiError::validation("currency must be a string or null")),
    }
}

fn patched_decimal(
    current: Option<Decimal>,
    patch: &PatchField,
    field: &str,
) -> ApiResult<Option<Decimal>> {
    if patch.is_set() {
        decimal_value(patch.value().unwrap_or(&Value::Null), field)
    } else {
        Ok(current)
    }
}

fn patched_bool(current: bool, patch: &PatchField, field: &str) -> ApiResult<bool> {
    if !patch.is_set() {
        return Ok(current);
    }
    match patch.value() {
        Some(Value::Bool(value)) => Ok(*value),
        _ => Err(ApiError::validation(format!("{field} must be a boolean"))),
    }
}

fn patched_document_kind(current: &str, patch: &PatchField) -> ApiResult<DocumentKind> {
    if !patch.is_set() {
        return parse_document_kind(current)
            .ok_or_else(|| ApiError::internal("document kind", "stored document type is invalid"));
    }
    let Some(Value::String(value)) = patch.value() else {
        return Err(ApiError::validation("document_type must be a string"));
    };
    parse_document_kind(value.trim()).ok_or_else(|| ApiError::validation("Unknown document_type."))
}

fn parse_document_kind(value: &str) -> Option<DocumentKind> {
    Some(match value {
        "supplier_invoice" => DocumentKind::SupplierInvoice,
        "credit_note" => DocumentKind::CreditNote,
        "receipt" => DocumentKind::Receipt,
        "purchase_order" => DocumentKind::PurchaseOrder,
        "statement" => DocumentKind::Statement,
        "other_financial" => DocumentKind::OtherFinancial,
        "unknown" => DocumentKind::Unknown,
        _ => return None,
    })
}

struct PatchedDate {
    date: Option<NaiveDate>,
    text: Option<String>,
}

fn patched_date(
    current_date: Option<NaiveDate>,
    current_text: Option<String>,
    patch: &PatchField,
    field: &str,
) -> ApiResult<PatchedDate> {
    if !patch.is_set() {
        return Ok(PatchedDate {
            date: current_date,
            text: current_text,
        });
    }
    match patch.value() {
        Some(Value::Null) => Ok(PatchedDate {
            date: None,
            text: None,
        }),
        Some(Value::String(raw)) if raw.trim().is_empty() => Ok(PatchedDate {
            date: None,
            text: None,
        }),
        Some(Value::String(raw)) => {
            let parsed = parse_invoice_date(raw)
                .map_err(|_| ApiError::validation(format!("{field} is not a valid date")))?;
            Ok(PatchedDate {
                date: Some(parsed.date),
                text: Some(parsed.original),
            })
        }
        _ => Err(ApiError::validation(format!(
            "{field} must be a date string or null"
        ))),
    }
}

fn patched_labels(current: Vec<String>, patch: &PatchField) -> ApiResult<Vec<String>> {
    if !patch.is_set() {
        return Ok(current);
    }
    let Some(Value::Array(values)) = patch.value() else {
        return Err(ApiError::validation("labels must be an array of strings"));
    };
    if values.len() > 20 {
        return Err(ApiError::validation("At most 20 labels are allowed."));
    }
    let mut labels = Vec::new();
    for value in values {
        let Some(raw) = value.as_str() else {
            return Err(ApiError::validation("Every label must be a string."));
        };
        let label = required_text(raw, "label", 1, 40)?;
        if !labels
            .iter()
            .any(|existing: &String| existing.eq_ignore_ascii_case(&label))
        {
            labels.push(label);
        }
    }
    Ok(labels)
}

fn parse_line_items(patch: &PatchField) -> ApiResult<Vec<db_invoices::NewLineItem>> {
    let Some(Value::Array(items)) = patch.value() else {
        return Err(ApiError::validation("line_items must be an array"));
    };
    if items.len() > 500 {
        return Err(ApiError::validation("At most 500 line items are allowed."));
    }
    let mut output = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        let Some(object) = item.as_object() else {
            return Err(ApiError::validation(format!(
                "line_items[{index}] must be an object"
            )));
        };
        let source_page = parse_source_page(object.get("source_page"), index)?;
        output.push(db_invoices::NewLineItem {
            description: optional_text(object.get("description"), "description", 500)?,
            sku: optional_text(object.get("sku"), "sku", 120)?,
            quantity: decimal_field(object.get("quantity"), "quantity")?,
            unit_of_measure: optional_text(object.get("unit_of_measure"), "unit_of_measure", 40)?,
            unit_price: decimal_field(object.get("unit_price"), "unit_price")?,
            discount: decimal_field(object.get("discount"), "discount")?,
            tax_rate: decimal_field(object.get("tax_rate"), "tax_rate")?,
            tax_amount: decimal_field(object.get("tax_amount"), "tax_amount")?,
            line_total: decimal_field(object.get("line_total"), "line_total")?,
            source_page,
            raw_text: optional_text(object.get("raw_text"), "raw_text", 2_000)?,
        });
    }
    Ok(output)
}

fn parse_tax_lines(patch: &PatchField) -> ApiResult<Vec<db_invoices::NewTaxLine>> {
    let Some(Value::Array(items)) = patch.value() else {
        return Err(ApiError::validation("tax_lines must be an array"));
    };
    if items.len() > 100 {
        return Err(ApiError::validation("At most 100 tax lines are allowed."));
    }
    let mut output = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        let Some(object) = item.as_object() else {
            return Err(ApiError::validation(format!(
                "tax_lines[{index}] must be an object"
            )));
        };
        output.push(db_invoices::NewTaxLine {
            tax_type: optional_text(object.get("tax_type"), "tax_type", 40)?,
            rate: decimal_field(object.get("rate"), "rate")?,
            taxable_amount: decimal_field(object.get("taxable_amount"), "taxable_amount")?,
            amount: decimal_field(object.get("amount"), "amount")?,
            source_page: parse_source_page(object.get("source_page"), index)?,
        });
    }
    Ok(output)
}

fn decimal_field(value: Option<&Value>, field: &str) -> ApiResult<Option<Decimal>> {
    match value {
        None => Ok(None),
        Some(value) => decimal_value(value, field),
    }
}

fn parse_source_page(value: Option<&Value>, index: usize) -> ApiResult<Option<i32>> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(number)) => {
            let page = number.as_i64().ok_or_else(|| {
                ApiError::validation(format!(
                    "line_items[{index}].source_page must be an integer"
                ))
            })?;
            if !(1..=10_000).contains(&page) {
                return Err(ApiError::validation("source_page is out of range"));
            }
            Ok(Some(page as i32))
        }
        _ => Err(ApiError::validation(
            "source_page must be an integer or null",
        )),
    }
}

fn line_item_from_row(row: &db_invoices::LineItemRow) -> db_invoices::NewLineItem {
    db_invoices::NewLineItem {
        description: row.description.clone(),
        sku: row.sku.clone(),
        quantity: row.quantity,
        unit_of_measure: row.unit_of_measure.clone(),
        unit_price: row.unit_price,
        discount: row.discount,
        tax_rate: row.tax_rate,
        tax_amount: row.tax_amount,
        line_total: row.line_total,
        source_page: row.source_page,
        raw_text: row.raw_text.clone(),
    }
}

fn tax_line_from_row(row: &db_invoices::TaxLineRow) -> db_invoices::NewTaxLine {
    db_invoices::NewTaxLine {
        tax_type: row.tax_type.clone(),
        rate: row.rate,
        taxable_amount: row.taxable_amount,
        amount: row.amount,
        source_page: row.source_page,
    }
}

fn field_status(field: &str, issues: &[ValidationIssue]) -> String {
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
    existing: &[db_invoices::ProvenanceRow],
    validation_issues: &[ValidationIssue],
    all_issues: &[(String, String, Option<String>, String)],
    actor: Uuid,
    fields: &[(&str, Option<String>)],
) -> Vec<db_invoices::NewProvenance> {
    let by_field: std::collections::HashMap<&str, &db_invoices::ProvenanceRow> = existing
        .iter()
        .map(|row| (row.field_path.as_str(), row))
        .collect();
    fields
        .iter()
        .map(|(field, normalized)| {
            let prior = by_field.get(field);
            let status = field_status(field, validation_issues);
            let mut status = status;
            if status == "valid" {
                if let Some(issue) = all_issues
                    .iter()
                    .find(|issue| issue.2.as_deref() == Some(field))
                {
                    status = if issue.1 == "error" {
                        "error".into()
                    } else if issue.1 == "warning" {
                        "warning".into()
                    } else {
                        "info".into()
                    };
                }
            }
            db_invoices::NewProvenance {
                field_path: (*field).to_string(),
                extracted_value: prior
                    .and_then(|row| row.extracted_value.clone())
                    .or_else(|| normalized.clone()),
                normalized_value: normalized.clone(),
                confidence: prior.and_then(|row| row.confidence),
                extraction_method: prior
                    .and_then(|row| row.extraction_method.clone())
                    .or_else(|| Some("human".into())),
                source_page: prior.and_then(|row| row.source_page),
                source_text: prior.and_then(|row| row.source_text.clone()),
                bbox: prior.and_then(|row| row.bbox.clone()),
                validation_status: status,
                human_edited: true,
                previous_value: prior.and_then(|row| row.normalized_value.clone()),
                edited_by: Some(actor),
            }
        })
        .collect()
}

// Silence the unused-import warning for map helpers while keeping the module
// prepared for richer patch diffs.
#[allow(dead_code)]
fn empty_changes() -> Map<String, Value> {
    Map::new()
}
