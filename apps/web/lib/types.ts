export type Role = "owner" | "admin" | "accountant" | "reviewer" | "read_only";

export interface Organization {
  id: string;
  name: string;
  slug: string;
  country: string;
  base_currency: string;
  is_demo: boolean;
  settings: Record<string, unknown>;
}

export interface Membership {
  organization: Organization;
  role: Role;
  permissions: string[];
}

export interface Session {
  user: { id: string; email: string; name: string } | null;
  auth: {
    kind: "session" | "api_key";
    session_expires_at: string | null;
    scopes: string[] | null;
  };
  memberships: Membership[];
  active_organization: {
    organization: Organization;
    role: Role | null;
    permissions: string[];
  };
  csrf_token: string | null;
}

export interface InvoiceListRow {
  id: string;
  document_id: string;
  vendor_id: string | null;
  vendor_name: string | null;
  vendor_tax_id?: string | null;
  invoice_number: string | null;
  invoice_date: string | null;
  due_date: string | null;
  currency: string | null;
  total: string | null;
  status: string;
  validation_status?: string;
  review_required?: boolean;
  is_mock: boolean;
  provider: string | null;
  issue_count: number;
  blocking_issue_count: number;
  duplicate_count: number;
  open_duplicate_count: number;
  version: number;
  source_filename?: string | null;
  created_at: string;
  updated_at: string;
  assigned_to_name?: string | null;
}

export interface ValidationIssue {
  id: string;
  code: string;
  field: string | null;
  severity: "error" | "warning" | "info";
  message: string;
  resolved_at: string | null;
  resolution_note: string | null;
}

export interface DuplicateRow {
  id: string;
  kind: string;
  status: "open" | "dismissed" | "confirmed";
  score: string | null;
  details: Record<string, unknown> | null;
  resolution_note: string | null;
  resolved_at: string | null;
  other: {
    id: string;
    invoice_number: string | null;
    vendor_name: string | null;
    total: string | null;
    currency: string | null;
    invoice_date?: string | null;
    status: string;
    created_at: string;
  };
}

export interface LineItem {
  id?: string;
  position?: number;
  description: string | null;
  sku: string | null;
  quantity: string | null;
  unit_of_measure: string | null;
  unit_price: string | null;
  discount: string | null;
  tax_rate: string | null;
  tax_amount: string | null;
  line_total: string | null;
  source_page?: number | null;
  raw_text?: string | null;
}

export interface TaxLine {
  id?: string;
  position?: number;
  tax_type: string | null;
  rate: string | null;
  taxable_amount: string | null;
  amount: string | null;
  source_page?: number | null;
}

export interface ProvenanceRow {
  id: string;
  field: string;
  extracted_value: string | null;
  normalized_value: string | null;
  confidence: string | null;
  extraction_method: string | null;
  source_page: number | null;
  source_text: string | null;
  validation_status: string | null;
  human_edited: boolean;
  previous_value: string | null;
  edited_at: string | null;
}

export interface ReviewRow {
  id: string;
  action: string;
  reviewer_name: string | null;
  notes: string | null;
  changed_fields: string[];
  created_at: string;
}

export interface ApprovalEvent {
  id: string;
  action: string;
  actor_name: string | null;
  reason: string | null;
  created_at: string;
}

export interface InvoiceDetail extends InvoiceListRow {
  subtotal: string | null;
  discount: string | null;
  shipping: string | null;
  tax_total: string | null;
  amount_paid: string | null;
  balance_due: string | null;
  purchase_order: string | null;
  payment_terms: string | null;
  document_type: string | { value: string; label: string; is_invoice_like: boolean };
  invoice_date_text: string | null;
  due_date_text: string | null;
  is_credit_note: boolean;
  extraction: {
    provider: string | null;
    model: string | null;
    method: string | null;
    quality: string | null;
    is_mock: boolean;
  };
  labels: string[];
  assigned_reviewer_name: string | null;
  available_actions: {
    edit: boolean;
    approve: boolean;
    reject: boolean;
    return_for_correction: boolean;
    export: boolean;
    retry_processing: boolean;
  };
  line_items: LineItem[];
  tax_lines: TaxLine[];
  provenance: ProvenanceRow[];
  issues: ValidationIssue[];
  duplicates: DuplicateRow[];
  reviews: ReviewRow[];
  approval_events: ApprovalEvent[];
  blocking: {
    approval_blocked: boolean;
    error_count: number;
    warning_count: number;
    open_duplicates: number;
    confirmed_duplicates: number;
    mock_requires_acknowledgement: boolean;
  };
  document: {
    id: string;
    filename: string;
    status: string;
    content_type: string;
    content_url: string;
  } | null;
}

export interface DocumentTypeInfo {
  value: string;
  label: string;
  is_invoice_like: boolean;
}

export interface DocumentInvoiceSummary {
  id: string;
  vendor_name: string | null;
  invoice_number: string | null;
  currency: string | null;
  total: string | null;
  status: string;
  issue_count: number;
  duplicate_count: number;
}

export interface DocumentRow {
  id: string;
  filename: string;
  content_type: string;
  size_bytes: number;
  page_count: number | null;
  document_type: DocumentTypeInfo;
  status: string;
  error_code: string | null;
  created_at: string;
  invoice: DocumentInvoiceSummary | null;
}

export interface DocumentDetail extends DocumentRow {
  source: string;
  sha256: string;
  processing_attempts: number;
  extraction_provider: string | null;
  extraction_model: string | null;
  processing_duration_ms: number | null;
  error_message: string | null;
  updated_at: string;
  content_url: string;
  download_url_endpoint: string;
  events: DocumentEvent[];
  jobs: ProcessingJob[];
}

export interface DocumentEvent {
  id: string;
  stage: string;
  status: string;
  message: string;
  duration_ms: number | null;
  created_at: string;
}

export interface ProcessingJob {
  id: string;
  status: string;
  attempts: number;
  max_attempts: number;
  run_at: string;
  stage: string | null;
  error_code: string | null;
  error_message: string | null;
  started_at: string | null;
  completed_at: string | null;
  created_at: string;
  updated_at: string;
}

export interface VendorRow {
  id: string;
  name: string;
  tax_id: string | null;
  email: string | null;
  phone: string | null;
  address: string | null;
  invoice_count: number;
  review_count: number;
  last_invoice_at: string | null;
  created_at: string;
}

export interface DashboardData {
  generated_at: string;
  period: { days: number; from: string; to: string };
  counts: {
    documents_received: number;
    invoices_extracted: number;
    awaiting_review: number;
    validation_failures: number;
    duplicate_candidates: number;
    approved_invoices: number;
    exported_invoices: number;
    processing_failures: number;
    pages_processed: number;
    avg_processing_ms: number;
    total_ai_requests: number;
  };
  document_status: Array<{ status: string; count: number }>;
  daily: Array<{ day: string; documents: number; approved: number }>;
  recent_activity: Array<{
    id: string;
    action: string;
    entity_type: string | null;
    entity_id: string | null;
    actor_name: string | null;
    metadata: Record<string, unknown>;
    created_at: string;
  }>;
  recent_errors: Array<{
    document_id: string;
    filename: string;
    error_code: string | null;
    error_message: string | null;
    created_at: string;
  }>;
}

export interface ExportJob {
  id: string;
  format: "csv" | "json";
  status: string;
  filters: Record<string, unknown>;
  row_count: number | null;
  filename: string | null;
  download_url: string | null;
  error_code: string | null;
  error_message: string | null;
  requested_by_name: string | null;
  created_at: string;
  started_at: string | null;
  completed_at: string | null;
  expires_at: string | null;
}

export interface AuditEvent {
  id: string;
  action: string;
  entity_type: string | null;
  entity_id: string | null;
  metadata: Record<string, unknown>;
  actor_name: string | null;
  actor_email: string | null;
  actor_key_name: string | null;
  ip: string | null;
  request_id: string | null;
  created_at: string;
}

export interface ApiKeyRow {
  id: string;
  name: string;
  prefix: string;
  scopes: string[];
  last_used_at: string | null;
  expires_at: string | null;
  revoked_at: string | null;
  created_at: string;
  created_by_name: string | null;
}

export interface WebhookEndpoint {
  id: string;
  url: string;
  description: string | null;
  events: string[];
  enabled: boolean;
  last_success_at: string | null;
  last_failure_at: string | null;
  delivery_count: number;
  failed_count: number;
  created_at: string;
  updated_at: string;
}

export interface WebhookDelivery {
  id: string;
  endpoint_id: string;
  endpoint_url: string;
  event_id: string;
  event_type: string;
  status: string;
  attempts: number;
  last_status_code: number | null;
  last_error: string | null;
  next_attempt_at: string | null;
  created_at: string;
  delivered_at: string | null;
}

export interface AutomationRule {
  id: string;
  name: string;
  description: string | null;
  enabled: boolean;
  priority: number;
  spec: {
    match_all: boolean;
    conditions: Array<{ field: string; op: string; value: unknown }>;
    actions: Array<{ action: string; value?: unknown }>;
  };
  created_at: string;
  updated_at: string;
}

export interface Member {
  user_id: string;
  name: string;
  email: string;
  role: Role;
  created_at: string;
  last_seen_at?: string | null;
}

export interface Invitation {
  id: string;
  email: string;
  role: Role;
  expires_at: string;
  accepted_at: string | null;
  created_at: string;
  invited_by_name: string | null;
}
