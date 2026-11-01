-- Invoxa initial schema. PostgreSQL 15+.
-- All timestamps are stored as UTC (timestamptz). Monetary values use exact
-- NUMERIC(18,4); binary floating point is never used for money.

CREATE TABLE users (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    email TEXT NOT NULL,
    password_hash TEXT NOT NULL,
    name TEXT NOT NULL,
    email_verified_at TIMESTAMPTZ,
    disabled_at TIMESTAMPTZ,
    failed_login_attempts INTEGER NOT NULL DEFAULT 0,
    locked_until TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT users_email_len CHECK (char_length(email) BETWEEN 3 AND 320),
    CONSTRAINT users_name_len CHECK (char_length(name) BETWEEN 1 AND 160)
);
CREATE UNIQUE INDEX users_email_lower_uq ON users (lower(email));

CREATE TABLE organizations (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name TEXT NOT NULL,
    slug TEXT NOT NULL,
    country TEXT NOT NULL DEFAULT 'IN',
    base_currency TEXT NOT NULL DEFAULT 'INR',
    settings JSONB NOT NULL DEFAULT '{}'::jsonb,
    is_demo BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at TIMESTAMPTZ,
    CONSTRAINT organizations_name_len CHECK (char_length(name) BETWEEN 1 AND 160),
    CONSTRAINT organizations_slug_fmt CHECK (slug ~ '^[a-z0-9][a-z0-9-]{1,62}$'),
    CONSTRAINT organizations_country_fmt CHECK (country ~ '^[A-Z]{2,3}$'),
    CONSTRAINT organizations_currency_fmt CHECK (base_currency ~ '^[A-Z]{3}$')
);
CREATE UNIQUE INDEX organizations_slug_uq ON organizations (slug) WHERE deleted_at IS NULL;

CREATE TABLE organization_members (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    organization_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    role TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT organization_members_role_chk CHECK (role IN ('owner','admin','accountant','reviewer','read_only')),
    CONSTRAINT organization_members_uq UNIQUE (organization_id, user_id)
);
CREATE INDEX organization_members_user_idx ON organization_members (user_id);
CREATE INDEX organization_members_org_role_idx ON organization_members (organization_id, role);

CREATE TABLE invitations (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    organization_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    email TEXT NOT NULL,
    role TEXT NOT NULL,
    token_hash TEXT NOT NULL UNIQUE,
    invited_by UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    expires_at TIMESTAMPTZ NOT NULL,
    accepted_at TIMESTAMPTZ,
    accepted_by UUID REFERENCES users(id) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT invitations_role_chk CHECK (role IN ('admin','accountant','reviewer','read_only'))
);
CREATE INDEX invitations_org_idx ON invitations (organization_id, created_at DESC);
CREATE INDEX invitations_email_idx ON invitations (lower(email));

CREATE TABLE sessions (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    token_hash TEXT NOT NULL UNIQUE,
    ip TEXT,
    user_agent TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_seen_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at TIMESTAMPTZ NOT NULL,
    revoked_at TIMESTAMPTZ
);
CREATE INDEX sessions_user_idx ON sessions (user_id);
CREATE INDEX sessions_expiry_idx ON sessions (expires_at);

CREATE TABLE password_reset_tokens (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    token_hash TEXT NOT NULL UNIQUE,
    expires_at TIMESTAMPTZ NOT NULL,
    used_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX password_reset_user_idx ON password_reset_tokens (user_id);

CREATE TABLE documents (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    organization_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    uploaded_by UUID REFERENCES users(id) ON DELETE SET NULL,
    source TEXT NOT NULL DEFAULT 'web',
    original_filename TEXT NOT NULL,
    storage_key TEXT NOT NULL UNIQUE,
    content_type TEXT NOT NULL,
    sha256 TEXT NOT NULL,
    size_bytes BIGINT NOT NULL,
    page_count INTEGER,
    document_type TEXT NOT NULL DEFAULT 'unknown',
    processing_status TEXT NOT NULL DEFAULT 'uploaded',
    processing_attempts INTEGER NOT NULL DEFAULT 0,
    extraction_provider TEXT,
    extraction_model TEXT,
    processing_duration_ms BIGINT,
    last_error_code TEXT,
    last_error_message TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at TIMESTAMPTZ,
    CONSTRAINT documents_source_chk CHECK (source IN ('web','api','email','folder')),
    CONSTRAINT documents_type_chk CHECK (document_type IN ('supplier_invoice','credit_note','receipt','purchase_order','statement','other_financial','unknown')),
    CONSTRAINT documents_status_chk CHECK (processing_status IN ('uploaded','queued','extracting_text','running_ocr','extracting_fields','validating','review_required','ready_for_approval','approved','exported','rejected','failed','cancelled')),
    CONSTRAINT documents_size_chk CHECK (size_bytes > 0),
    CONSTRAINT documents_sha_fmt CHECK (sha256 ~ '^[0-9a-f]{64}$')
);
CREATE INDEX documents_org_created_idx ON documents (organization_id, created_at DESC);
CREATE INDEX documents_org_status_idx ON documents (organization_id, processing_status);
CREATE INDEX documents_org_hash_idx ON documents (organization_id, sha256);
CREATE INDEX documents_processing_idx ON documents (processing_status) WHERE deleted_at IS NULL;

CREATE TABLE document_processing_jobs (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    organization_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    document_id UUID NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    status TEXT NOT NULL DEFAULT 'queued',
    attempts INTEGER NOT NULL DEFAULT 0,
    max_attempts INTEGER NOT NULL DEFAULT 5,
    run_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    locked_at TIMESTAMPTZ,
    lease_expires_at TIMESTAMPTZ,
    locked_by TEXT,
    stage TEXT,
    started_at TIMESTAMPTZ,
    completed_at TIMESTAMPTZ,
    last_error_code TEXT,
    last_error_message TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT dpj_status_chk CHECK (status IN ('queued','running','retry','succeeded','dead','cancelled')),
    CONSTRAINT dpj_attempts_chk CHECK (attempts >= 0 AND max_attempts BETWEEN 1 AND 20)
);
CREATE INDEX dpj_due_idx ON document_processing_jobs (run_at)
    WHERE status IN ('queued','retry');
CREATE INDEX dpj_doc_idx ON document_processing_jobs (document_id, created_at DESC);
CREATE UNIQUE INDEX dpj_active_uq ON document_processing_jobs (document_id)
    WHERE status IN ('queued','running','retry');

CREATE TABLE document_events (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    organization_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    document_id UUID NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    stage TEXT NOT NULL,
    status TEXT NOT NULL,
    message TEXT,
    duration_ms BIGINT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX document_events_doc_idx ON document_events (document_id, created_at);

CREATE TABLE vendors (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    organization_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    normalized_name TEXT NOT NULL,
    tax_id TEXT,
    tax_id_normalized TEXT,
    email TEXT,
    phone TEXT,
    address TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at TIMESTAMPTZ,
    CONSTRAINT vendors_name_len CHECK (char_length(name) BETWEEN 1 AND 300)
);
CREATE INDEX vendors_org_name_idx ON vendors (organization_id, normalized_name) WHERE deleted_at IS NULL;
CREATE UNIQUE INDEX vendors_org_tax_uq ON vendors (organization_id, tax_id_normalized)
    WHERE tax_id_normalized IS NOT NULL AND deleted_at IS NULL;

CREATE TABLE extracted_invoices (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    organization_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    document_id UUID NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    vendor_id UUID REFERENCES vendors(id) ON DELETE SET NULL,
    document_type TEXT NOT NULL DEFAULT 'unknown',
    invoice_number TEXT,
    invoice_number_normalized TEXT,
    invoice_date DATE,
    due_date DATE,
    invoice_date_text TEXT,
    due_date_text TEXT,
    purchase_order TEXT,
    currency TEXT,
    payment_terms TEXT,
    subtotal NUMERIC(18,4),
    discount NUMERIC(18,4),
    shipping NUMERIC(18,4),
    tax_total NUMERIC(18,4),
    total NUMERIC(18,4),
    amount_paid NUMERIC(18,4),
    balance_due NUMERIC(18,4),
    is_credit_note BOOLEAN NOT NULL DEFAULT FALSE,
    status TEXT NOT NULL DEFAULT 'review_required',
    version INTEGER NOT NULL DEFAULT 1,
    extraction_provider TEXT,
    extraction_model TEXT,
    extraction_method TEXT,
    extraction_quality NUMERIC(5,4),
    raw_extraction JSONB,
    labels TEXT[] NOT NULL DEFAULT '{}'::text[],
    assigned_reviewer_id UUID REFERENCES users(id) ON DELETE SET NULL,
    approved_at TIMESTAMPTZ,
    approved_by UUID REFERENCES users(id) ON DELETE SET NULL,
    rejected_at TIMESTAMPTZ,
    rejected_by UUID REFERENCES users(id) ON DELETE SET NULL,
    rejection_reason TEXT,
    exported_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT invoices_document_uq UNIQUE (document_id),
    CONSTRAINT invoices_type_chk CHECK (document_type IN ('supplier_invoice','credit_note','receipt','purchase_order','statement','other_financial','unknown')),
    CONSTRAINT invoices_status_chk CHECK (status IN ('review_required','ready_for_approval','approved','rejected','exported')),
    CONSTRAINT invoices_quality_chk CHECK (extraction_quality IS NULL OR (extraction_quality >= 0 AND extraction_quality <= 1)),
    CONSTRAINT invoices_version_chk CHECK (version >= 1)
);
CREATE INDEX invoices_org_status_idx ON extracted_invoices (organization_id, status);
CREATE INDEX invoices_org_created_idx ON extracted_invoices (organization_id, created_at DESC);
CREATE INDEX invoices_vendor_idx ON extracted_invoices (organization_id, vendor_id);
CREATE INDEX invoices_number_idx ON extracted_invoices (organization_id, invoice_number_normalized);
CREATE INDEX invoices_org_total_idx ON extracted_invoices (organization_id, total);
CREATE INDEX invoices_approval_idx ON extracted_invoices (organization_id, approved_at) WHERE approved_at IS NOT NULL;
CREATE INDEX invoices_reviewer_idx ON extracted_invoices (assigned_reviewer_id) WHERE assigned_reviewer_id IS NOT NULL;
CREATE INDEX invoices_labels_idx ON extracted_invoices USING GIN (labels);

CREATE TABLE invoice_line_items (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    organization_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    invoice_id UUID NOT NULL REFERENCES extracted_invoices(id) ON DELETE CASCADE,
    position INTEGER NOT NULL,
    description TEXT,
    sku TEXT,
    quantity NUMERIC(18,4),
    unit_of_measure TEXT,
    unit_price NUMERIC(18,4),
    discount NUMERIC(18,4),
    tax_rate NUMERIC(9,4),
    tax_amount NUMERIC(18,4),
    line_total NUMERIC(18,4),
    source_page INTEGER,
    raw_text TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT line_items_position_uq UNIQUE (invoice_id, position),
    CONSTRAINT line_items_position_chk CHECK (position >= 0)
);
CREATE INDEX line_items_invoice_idx ON invoice_line_items (invoice_id, position);

CREATE TABLE invoice_tax_lines (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    organization_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    invoice_id UUID NOT NULL REFERENCES extracted_invoices(id) ON DELETE CASCADE,
    position INTEGER NOT NULL,
    tax_type TEXT,
    rate NUMERIC(9,4),
    taxable_amount NUMERIC(18,4),
    amount NUMERIC(18,4),
    source_page INTEGER,
    CONSTRAINT tax_lines_position_uq UNIQUE (invoice_id, position)
);
CREATE INDEX tax_lines_invoice_idx ON invoice_tax_lines (invoice_id, position);

CREATE TABLE invoice_field_provenance (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    organization_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    invoice_id UUID NOT NULL REFERENCES extracted_invoices(id) ON DELETE CASCADE,
    field_path TEXT NOT NULL,
    extracted_value TEXT,
    normalized_value TEXT,
    confidence NUMERIC(5,4),
    extraction_method TEXT,
    source_page INTEGER,
    source_text TEXT,
    bbox JSONB,
    validation_status TEXT NOT NULL DEFAULT 'unvalidated',
    human_edited BOOLEAN NOT NULL DEFAULT FALSE,
    previous_value TEXT,
    edited_by UUID REFERENCES users(id) ON DELETE SET NULL,
    edited_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT provenance_field_uq UNIQUE (invoice_id, field_path)
);
CREATE INDEX provenance_invoice_idx ON invoice_field_provenance (invoice_id);

CREATE TABLE validation_issues (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    organization_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    invoice_id UUID NOT NULL REFERENCES extracted_invoices(id) ON DELETE CASCADE,
    document_id UUID NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    code TEXT NOT NULL,
    severity TEXT NOT NULL,
    field_path TEXT,
    message TEXT NOT NULL,
    resolved_at TIMESTAMPTZ,
    resolved_by UUID REFERENCES users(id) ON DELETE SET NULL,
    resolution_note TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT validation_severity_chk CHECK (severity IN ('info','warning','error'))
);
CREATE INDEX validation_invoice_idx ON validation_issues (invoice_id, resolved_at);
CREATE INDEX validation_org_idx ON validation_issues (organization_id, severity) WHERE resolved_at IS NULL;

CREATE TABLE duplicate_candidates (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    organization_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    invoice_id UUID NOT NULL REFERENCES extracted_invoices(id) ON DELETE CASCADE,
    duplicate_of_invoice_id UUID NOT NULL REFERENCES extracted_invoices(id) ON DELETE CASCADE,
    kind TEXT NOT NULL,
    score NUMERIC(5,4) NOT NULL,
    details JSONB NOT NULL DEFAULT '{}'::jsonb,
    status TEXT NOT NULL DEFAULT 'open',
    resolved_by UUID REFERENCES users(id) ON DELETE SET NULL,
    resolved_at TIMESTAMPTZ,
    resolution_note TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT duplicate_kind_chk CHECK (kind IN ('exact_file','probable_invoice','similar')),
    CONSTRAINT duplicate_status_chk CHECK (status IN ('open','confirmed','dismissed')),
    CONSTRAINT duplicate_not_self CHECK (invoice_id <> duplicate_of_invoice_id),
    CONSTRAINT duplicate_pair_uq UNIQUE (invoice_id, duplicate_of_invoice_id)
);
CREATE INDEX duplicates_invoice_idx ON duplicate_candidates (invoice_id, status);
CREATE INDEX duplicates_org_idx ON duplicate_candidates (organization_id, status);

CREATE TABLE invoice_reviews (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    organization_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    invoice_id UUID NOT NULL REFERENCES extracted_invoices(id) ON DELETE CASCADE,
    reviewer_id UUID REFERENCES users(id) ON DELETE SET NULL,
    action TEXT NOT NULL,
    notes TEXT,
    changed_fields JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT review_action_chk CHECK (action IN ('corrected','returned','submitted','field_edit','line_item_edit'))
);
CREATE INDEX invoice_reviews_invoice_idx ON invoice_reviews (invoice_id, created_at DESC);

CREATE TABLE approval_events (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    organization_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    invoice_id UUID NOT NULL REFERENCES extracted_invoices(id) ON DELETE CASCADE,
    actor_id UUID REFERENCES users(id) ON DELETE SET NULL,
    action TEXT NOT NULL,
    reason TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT approval_action_chk CHECK (action IN ('approved','rejected','returned','override_approved','export_requested'))
);
CREATE INDEX approval_events_invoice_idx ON approval_events (invoice_id, created_at DESC);

CREATE TABLE automation_rules (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    organization_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    description TEXT,
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    priority INTEGER NOT NULL DEFAULT 100,
    spec JSONB NOT NULL,
    created_by UUID REFERENCES users(id) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT automation_name_len CHECK (char_length(name) BETWEEN 1 AND 120)
);
CREATE INDEX automation_rules_org_idx ON automation_rules (organization_id, enabled, priority);

CREATE TABLE automation_runs (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    organization_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    rule_id UUID NOT NULL REFERENCES automation_rules(id) ON DELETE CASCADE,
    invoice_id UUID NOT NULL REFERENCES extracted_invoices(id) ON DELETE CASCADE,
    outcome JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX automation_runs_invoice_idx ON automation_runs (invoice_id);

CREATE TABLE integration_connections (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    organization_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    provider TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'not_configured',
    config JSONB NOT NULL DEFAULT '{}'::jsonb,
    encrypted_credentials BYTEA,
    last_verified_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT integration_status_chk CHECK (status IN ('not_configured','configured','connected','error','revoked')),
    CONSTRAINT integration_provider_uq UNIQUE (organization_id, provider)
);

CREATE TABLE api_keys (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    organization_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    prefix TEXT NOT NULL,
    key_hash TEXT NOT NULL UNIQUE,
    scopes TEXT[] NOT NULL DEFAULT '{}'::text[],
    created_by UUID REFERENCES users(id) ON DELETE SET NULL,
    last_used_at TIMESTAMPTZ,
    expires_at TIMESTAMPTZ,
    revoked_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT api_key_name_len CHECK (char_length(name) BETWEEN 1 AND 100)
);
CREATE INDEX api_keys_org_idx ON api_keys (organization_id, created_at DESC);
CREATE INDEX api_keys_prefix_idx ON api_keys (prefix);

CREATE TABLE webhook_endpoints (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    organization_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    url TEXT NOT NULL,
    description TEXT,
    secret_ciphertext BYTEA NOT NULL,
    events TEXT[] NOT NULL DEFAULT '{}'::text[],
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    created_by UUID REFERENCES users(id) ON DELETE SET NULL,
    last_success_at TIMESTAMPTZ,
    last_failure_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX webhook_endpoints_org_idx ON webhook_endpoints (organization_id, created_at DESC);

CREATE TABLE webhook_deliveries (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    organization_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    endpoint_id UUID NOT NULL REFERENCES webhook_endpoints(id) ON DELETE CASCADE,
    event_id UUID NOT NULL,
    event_type TEXT NOT NULL,
    payload JSONB NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending',
    attempts INTEGER NOT NULL DEFAULT 0,
    next_attempt_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_status_code INTEGER,
    last_error TEXT,
    response_snippet TEXT,
    locked_at TIMESTAMPTZ,
    lease_expires_at TIMESTAMPTZ,
    locked_by TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    delivered_at TIMESTAMPTZ,
    CONSTRAINT webhook_delivery_status_chk CHECK (status IN ('pending','delivering','succeeded','failed','dead_letter')),
    CONSTRAINT webhook_event_uq UNIQUE (endpoint_id, event_id)
);
CREATE INDEX webhook_deliveries_due_idx ON webhook_deliveries (next_attempt_at)
    WHERE status IN ('pending','failed');
CREATE INDEX webhook_deliveries_org_idx ON webhook_deliveries (organization_id, created_at DESC);
CREATE INDEX webhook_deliveries_endpoint_idx ON webhook_deliveries (endpoint_id, created_at DESC);

CREATE TABLE webhook_delivery_attempts (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    organization_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    delivery_id UUID NOT NULL REFERENCES webhook_deliveries(id) ON DELETE CASCADE,
    attempt_number INTEGER NOT NULL,
    status_code INTEGER,
    error TEXT,
    response_snippet TEXT,
    duration_ms BIGINT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX webhook_attempts_delivery_idx ON webhook_delivery_attempts (delivery_id, attempt_number);

CREATE TABLE export_jobs (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    organization_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    requested_by UUID REFERENCES users(id) ON DELETE SET NULL,
    format TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'queued',
    filters JSONB NOT NULL DEFAULT '{}'::jsonb,
    storage_key TEXT,
    filename TEXT,
    row_count INTEGER,
    error_code TEXT,
    error_message TEXT,
    idempotency_key TEXT,
    locked_at TIMESTAMPTZ,
    lease_expires_at TIMESTAMPTZ,
    locked_by TEXT,
    attempts INTEGER NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    started_at TIMESTAMPTZ,
    completed_at TIMESTAMPTZ,
    expires_at TIMESTAMPTZ,
    CONSTRAINT export_format_chk CHECK (format IN ('csv','json')),
    CONSTRAINT export_status_chk CHECK (status IN ('queued','running','succeeded','failed'))
);
CREATE INDEX export_jobs_org_idx ON export_jobs (organization_id, created_at DESC);
CREATE INDEX export_jobs_due_idx ON export_jobs (created_at) WHERE status = 'queued';
CREATE UNIQUE INDEX export_jobs_idempotency_uq ON export_jobs (organization_id, idempotency_key)
    WHERE idempotency_key IS NOT NULL;

CREATE TABLE audit_events (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    organization_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    actor_user_id UUID REFERENCES users(id) ON DELETE SET NULL,
    actor_api_key_id UUID REFERENCES api_keys(id) ON DELETE SET NULL,
    action TEXT NOT NULL,
    entity_type TEXT,
    entity_id UUID,
    metadata JSONB NOT NULL DEFAULT '{}'::jsonb,
    ip TEXT,
    request_id TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX audit_events_org_idx ON audit_events (organization_id, created_at DESC);
CREATE INDEX audit_events_entity_idx ON audit_events (organization_id, entity_type, entity_id);
CREATE INDEX audit_events_action_idx ON audit_events (organization_id, action);

CREATE TABLE usage_records (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    organization_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    document_id UUID REFERENCES documents(id) ON DELETE SET NULL,
    kind TEXT NOT NULL,
    quantity NUMERIC(18,4) NOT NULL DEFAULT 1,
    provider TEXT,
    model TEXT,
    input_tokens INTEGER,
    output_tokens INTEGER,
    duration_ms BIGINT,
    estimated_cost_usd NUMERIC(14,6),
    metadata JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT usage_kind_chk CHECK (kind IN ('document_upload','page_processed','ocr_page','ai_extraction','api_request','storage'))
);
CREATE INDEX usage_org_created_idx ON usage_records (organization_id, created_at DESC);
CREATE INDEX usage_org_kind_idx ON usage_records (organization_id, kind, created_at DESC);

-- A single-row operational lock prevents duplicate seed/demo initialization.
CREATE TABLE system_locks (
    key TEXT PRIMARY KEY,
    locked_until TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
