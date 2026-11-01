use crate::{Db, DbError};
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use sqlx::FromRow;
use uuid::Uuid;

#[derive(Debug, Clone, FromRow)]
pub struct VendorRow {
    pub id: Uuid,
    pub organization_id: Uuid,
    pub name: String,
    pub normalized_name: String,
    pub tax_id: Option<String>,
    pub tax_id_normalized: Option<String>,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub address: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, FromRow)]
pub struct VendorListRow {
    pub id: Uuid,
    pub name: String,
    pub tax_id: Option<String>,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub address: Option<String>,
    pub invoice_count: i64,
    pub review_count: i64,
    pub last_invoice_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, FromRow)]
pub struct VendorInvoiceRow {
    pub id: Uuid,
    pub invoice_number: Option<String>,
    pub invoice_date: Option<chrono::NaiveDate>,
    pub currency: Option<String>,
    pub total: Option<Decimal>,
    pub status: String,
    pub document_id: Uuid,
    pub created_at: DateTime<Utc>,
}

pub async fn list_vendors(
    db: &Db,
    organization_id: Uuid,
    search: Option<&str>,
    limit: i64,
    offset: i64,
) -> Result<(Vec<VendorListRow>, i64), sqlx::Error> {
    let rows = sqlx::query_as::<_, VendorListRow>(
        r#"SELECT v.id, v.name, v.tax_id, v.email, v.phone, v.address,
                  COALESCE((SELECT count(*) FROM extracted_invoices i
                            WHERE i.vendor_id = v.id AND i.organization_id = v.organization_id), 0) AS invoice_count,
                  COALESCE((SELECT count(*) FROM extracted_invoices i
                            WHERE i.vendor_id = v.id AND i.organization_id = v.organization_id
                              AND i.status = 'review_required'), 0) AS review_count,
                  (SELECT max(i.created_at) FROM extracted_invoices i
                   WHERE i.vendor_id = v.id AND i.organization_id = v.organization_id) AS last_invoice_at,
                  v.created_at
           FROM vendors v
           WHERE v.organization_id = $1 AND v.deleted_at IS NULL
             AND ($2::text IS NULL OR v.name ILIKE '%' || $2 || '%' OR v.tax_id ILIKE '%' || $2 || '%')
           ORDER BY v.name
           LIMIT $3 OFFSET $4"#,
    )
    .bind(organization_id)
    .bind(search)
    .bind(limit)
    .bind(offset)
    .fetch_all(db)
    .await?;
    let (total,): (i64,) = sqlx::query_as(
        "SELECT count(*) FROM vendors WHERE organization_id = $1 AND deleted_at IS NULL AND ($2::text IS NULL OR name ILIKE '%' || $2 || '%' OR tax_id ILIKE '%' || $2 || '%')",
    )
    .bind(organization_id)
    .bind(search)
    .fetch_one(db)
    .await?;
    Ok((rows, total))
}

pub async fn get_vendor(
    db: &Db,
    organization_id: Uuid,
    vendor_id: Uuid,
) -> Result<Option<VendorRow>, sqlx::Error> {
    sqlx::query_as::<_, VendorRow>(
        r#"SELECT id, organization_id, name, normalized_name, tax_id, tax_id_normalized, email,
                  phone, address, created_at, updated_at
           FROM vendors WHERE organization_id = $1 AND id = $2 AND deleted_at IS NULL"#,
    )
    .bind(organization_id)
    .bind(vendor_id)
    .fetch_optional(db)
    .await
}

pub async fn create_vendor(
    db: &Db,
    organization_id: Uuid,
    name: &str,
    tax_id: Option<&str>,
    email: Option<&str>,
    phone: Option<&str>,
    address: Option<&str>,
) -> Result<VendorRow, DbError> {
    let normalized_name = invoxa_domain::normalization::vendor_match_key(name);
    let tax_id_normalized = tax_id
        .map(invoxa_domain::normalization::normalize_tax_id)
        .filter(|s| !s.is_empty());
    let row = sqlx::query_as::<_, VendorRow>(
        r#"INSERT INTO vendors (organization_id, name, normalized_name, tax_id, tax_id_normalized, email, phone, address)
           VALUES ($1,$2,$3,$4,$5,$6,$7,$8)
           RETURNING id, organization_id, name, normalized_name, tax_id, tax_id_normalized, email,
                     phone, address, created_at, updated_at"#,
    )
    .bind(organization_id)
    .bind(name.trim())
    .bind(&normalized_name)
    .bind(tax_id.map(str::trim).filter(|s| !s.is_empty()))
    .bind(&tax_id_normalized)
    .bind(email.map(str::trim).filter(|s| !s.is_empty()))
    .bind(phone.map(str::trim).filter(|s| !s.is_empty()))
    .bind(address.map(str::trim).filter(|s| !s.is_empty()))
    .fetch_one(db)
    .await
    .map_err(|e| {
        if matches!(&e, sqlx::Error::Database(d) if d.is_unique_violation()) {
            DbError::Conflict("a vendor with this tax identifier already exists".into())
        } else {
            DbError::Sqlx(e)
        }
    })?;
    Ok(row)
}

pub async fn update_vendor(
    db: &Db,
    organization_id: Uuid,
    vendor_id: Uuid,
    name: &str,
    tax_id: Option<&str>,
    email: Option<&str>,
    phone: Option<&str>,
    address: Option<&str>,
) -> Result<VendorRow, DbError> {
    let normalized_name = invoxa_domain::normalization::vendor_match_key(name);
    let tax_id_normalized = tax_id
        .map(invoxa_domain::normalization::normalize_tax_id)
        .filter(|s| !s.is_empty());
    sqlx::query_as::<_, VendorRow>(
        r#"UPDATE vendors SET name = $3, normalized_name = $4, tax_id = $5, tax_id_normalized = $6,
                  email = $7, phone = $8, address = $9, updated_at = now()
           WHERE organization_id = $1 AND id = $2 AND deleted_at IS NULL
           RETURNING id, organization_id, name, normalized_name, tax_id, tax_id_normalized, email,
                     phone, address, created_at, updated_at"#,
    )
    .bind(organization_id)
    .bind(vendor_id)
    .bind(name.trim())
    .bind(&normalized_name)
    .bind(tax_id.map(str::trim).filter(|s| !s.is_empty()))
    .bind(&tax_id_normalized)
    .bind(email.map(str::trim).filter(|s| !s.is_empty()))
    .bind(phone.map(str::trim).filter(|s| !s.is_empty()))
    .bind(address.map(str::trim).filter(|s| !s.is_empty()))
    .fetch_optional(db)
    .await?
    .ok_or(DbError::NotFound)
}

/// Used by the extraction pipeline. It only auto-links on an exact normalized
/// tax identifier or an exact normalized name; fuzzy matches are suggested in
/// the UI and never merged automatically.
pub async fn find_or_create_vendor(
    db: &Db,
    organization_id: Uuid,
    name: Option<&str>,
    tax_id: Option<&str>,
) -> Result<Option<VendorRow>, DbError> {
    let Some(name) = name.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(None);
    };
    let tax_norm = tax_id
        .map(invoxa_domain::normalization::normalize_tax_id)
        .filter(|s| !s.is_empty());
    if let Some(tax_norm) = &tax_norm {
        let found = sqlx::query_as::<_, VendorRow>(
            r#"SELECT id, organization_id, name, normalized_name, tax_id, tax_id_normalized, email,
                      phone, address, created_at, updated_at
               FROM vendors WHERE organization_id = $1 AND tax_id_normalized = $2 AND deleted_at IS NULL"#,
        )
        .bind(organization_id)
        .bind(tax_norm)
        .fetch_optional(db)
        .await?;
        if let Some(vendor) = found {
            return Ok(Some(vendor));
        }
    }
    let normalized = invoxa_domain::normalization::vendor_match_key(name);
    let found = sqlx::query_as::<_, VendorRow>(
        r#"SELECT id, organization_id, name, normalized_name, tax_id, tax_id_normalized, email,
                  phone, address, created_at, updated_at
           FROM vendors WHERE organization_id = $1 AND normalized_name = $2 AND deleted_at IS NULL
           ORDER BY created_at LIMIT 1"#,
    )
    .bind(organization_id)
    .bind(&normalized)
    .fetch_optional(db)
    .await?;
    if let Some(vendor) = found {
        return Ok(Some(vendor));
    }
    match create_vendor(db, organization_id, name, tax_id, None, None, None).await {
        Ok(vendor) => Ok(Some(vendor)),
        Err(DbError::Conflict(_)) => {
            let vendor = sqlx::query_as::<_, VendorRow>(
                r#"SELECT id, organization_id, name, normalized_name, tax_id, tax_id_normalized, email,
                          phone, address, created_at, updated_at
                   FROM vendors WHERE organization_id = $1 AND tax_id_normalized = $2 AND deleted_at IS NULL"#,
            )
            .bind(organization_id)
            .bind(&tax_norm)
            .fetch_optional(db)
            .await?;
            Ok(vendor)
        }
        Err(e) => Err(e),
    }
}

#[derive(Debug, Clone, FromRow)]
pub struct VendorSuggestion {
    pub id: Uuid,
    pub name: String,
    pub tax_id: Option<String>,
    pub similarity: f64,
}

pub async fn suggest_vendors(
    db: &Db,
    organization_id: Uuid,
    query: &str,
    limit: i64,
) -> Result<Vec<VendorSuggestion>, sqlx::Error> {
    let normalized = invoxa_domain::normalization::vendor_match_key(query);
    if normalized.is_empty() {
        return Ok(Vec::new());
    }
    sqlx::query_as::<_, VendorSuggestion>(
        r#"SELECT id, name, tax_id,
                  CASE WHEN normalized_name = $2 THEN 1.0::float8
                       WHEN normalized_name LIKE $2 || '%' THEN 0.8::float8
                       ELSE 0.5::float8 END AS similarity
           FROM vendors
           WHERE organization_id = $1 AND deleted_at IS NULL
             AND (normalized_name LIKE '%' || $2 || '%' OR tax_id_normalized = $3)
           ORDER BY similarity DESC, name
           LIMIT $4"#,
    )
    .bind(organization_id)
    .bind(&normalized)
    .bind(invoxa_domain::normalization::normalize_tax_id(query))
    .bind(limit)
    .fetch_all(db)
    .await
}

pub async fn vendor_invoices(
    db: &Db,
    organization_id: Uuid,
    vendor_id: Uuid,
    limit: i64,
    offset: i64,
) -> Result<Vec<VendorInvoiceRow>, sqlx::Error> {
    sqlx::query_as::<_, VendorInvoiceRow>(
        r#"SELECT i.id, i.invoice_number, i.invoice_date, i.currency, i.total, i.status,
                  i.document_id, i.created_at
           FROM extracted_invoices i
           WHERE i.organization_id = $1 AND i.vendor_id = $2
           ORDER BY i.invoice_date DESC NULLS LAST, i.created_at DESC
           LIMIT $3 OFFSET $4"#,
    )
    .bind(organization_id)
    .bind(vendor_id)
    .bind(limit)
    .bind(offset)
    .fetch_all(db)
    .await
}

pub async fn assign_vendor_to_invoice(
    db: &Db,
    organization_id: Uuid,
    invoice_id: Uuid,
    vendor_id: Option<Uuid>,
) -> Result<(), DbError> {
    if let Some(vendor_id) = vendor_id {
        let exists: Option<(i32,)> = sqlx::query_as(
            "SELECT 1 FROM vendors WHERE organization_id = $1 AND id = $2 AND deleted_at IS NULL",
        )
        .bind(organization_id)
        .bind(vendor_id)
        .fetch_optional(db)
        .await?;
        if exists.is_none() {
            return Err(DbError::Validation(
                "vendor does not belong to this organization".into(),
            ));
        }
    }
    let result = sqlx::query(
        "UPDATE extracted_invoices SET vendor_id = $3, updated_at = now() WHERE organization_id = $1 AND id = $2",
    )
    .bind(organization_id)
    .bind(invoice_id)
    .bind(vendor_id)
    .execute(db)
    .await?;
    if result.rows_affected() == 0 {
        return Err(DbError::NotFound);
    }
    Ok(())
}
