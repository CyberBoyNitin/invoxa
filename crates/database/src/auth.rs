use crate::{slugify, Db, DbError};
use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::{FromRow, Postgres, Transaction};
use uuid::Uuid;

#[derive(Debug, Clone, FromRow)]
pub struct UserRow {
    pub id: Uuid,
    pub email: String,
    pub password_hash: String,
    pub name: String,
    pub email_verified_at: Option<DateTime<Utc>>,
    pub disabled_at: Option<DateTime<Utc>>,
    pub failed_login_attempts: i32,
    pub locked_until: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, FromRow)]
pub struct OrganizationRow {
    pub id: Uuid,
    pub name: String,
    pub slug: String,
    pub country: String,
    pub base_currency: String,
    pub settings: Value,
    pub is_demo: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, FromRow)]
pub struct MembershipContext {
    pub organization_id: Uuid,
    pub organization_name: String,
    pub organization_slug: String,
    pub role: String,
    pub country: String,
    pub base_currency: String,
    pub is_demo: bool,
    pub settings: Value,
}

#[derive(Debug, Clone, FromRow)]
pub struct SessionUserRow {
    pub session_id: Uuid,
    pub user_id: Uuid,
    pub email: String,
    pub name: String,
    pub session_expires_at: DateTime<Utc>,
    pub user_created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, FromRow)]
pub struct MemberRow {
    pub user_id: Uuid,
    pub email: String,
    pub name: String,
    pub role: String,
    pub created_at: DateTime<Utc>,
    pub last_seen_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, FromRow)]
pub struct InvitationRow {
    pub id: Uuid,
    pub organization_id: Uuid,
    pub email: String,
    pub role: String,
    pub invited_by_name: String,
    pub expires_at: DateTime<Utc>,
    pub accepted_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

async fn slug_in_tx(tx: &mut Transaction<'_, Postgres>, base: &str) -> Result<String, sqlx::Error> {
    let base = slugify(base);
    for attempt in 0..50 {
        let candidate = if attempt == 0 {
            base.clone()
        } else {
            format!("{base}-{attempt}")
        };
        let exists: Option<(i32,)> =
            sqlx::query_as("SELECT 1 FROM organizations WHERE slug = $1 AND deleted_at IS NULL")
                .bind(&candidate)
                .fetch_optional(&mut **tx)
                .await?;
        if exists.is_none() {
            return Ok(candidate);
        }
    }
    Ok(format!("{base}-{}", Uuid::new_v4().simple()))
}

pub async fn register_user_and_org(
    db: &Db,
    email: &str,
    password_hash: &str,
    name: &str,
    org_name: &str,
    country: &str,
    base_currency: &str,
    is_demo: bool,
) -> Result<(UserRow, OrganizationRow), DbError> {
    let mut tx = db.begin().await?;
    let normalized_email = email.trim().to_lowercase();
    let user = sqlx::query_as::<_, UserRow>(
        r#"INSERT INTO users (email, password_hash, name)
           VALUES ($1, $2, $3)
           RETURNING id, email, password_hash, name, email_verified_at, disabled_at,
                     failed_login_attempts, locked_until, created_at, updated_at"#,
    )
    .bind(&normalized_email)
    .bind(password_hash)
    .bind(name.trim())
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| {
        if matches!(&e, sqlx::Error::Database(d) if d.is_unique_violation()) {
            DbError::Conflict("an account with this email already exists".into())
        } else {
            DbError::Sqlx(e)
        }
    })?;

    let slug = slug_in_tx(&mut tx, org_name).await?;
    let org = sqlx::query_as::<_, OrganizationRow>(
        r#"INSERT INTO organizations (name, slug, country, base_currency, is_demo, settings)
           VALUES ($1, $2, $3, $4, $5, $6)
           RETURNING id, name, slug, country, base_currency, settings, is_demo, created_at, updated_at"#,
    )
    .bind(org_name.trim())
    .bind(&slug)
    .bind(country)
    .bind(base_currency)
    .bind(is_demo)
    .bind(serde_json::json!({
        "validation": {
            "require_invoice_date": true,
            "require_po_number": false,
            "require_vendor_tax_id": false,
            "require_gstin_for_india": false,
            "amount_tolerance": "0.02",
            "max_due_date_days": 730
        },
        "review": { "require_duplicate_resolution": true },
        "retention_days": null
    }))
    .fetch_one(&mut *tx)
    .await?;

    sqlx::query(
        "INSERT INTO organization_members (organization_id, user_id, role) VALUES ($1, $2, 'owner')",
    )
    .bind(org.id)
    .bind(user.id)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok((user, org))
}

pub async fn find_user_by_email(db: &Db, email: &str) -> Result<Option<UserRow>, sqlx::Error> {
    sqlx::query_as::<_, UserRow>(
        r#"SELECT id, email, password_hash, name, email_verified_at, disabled_at,
                  failed_login_attempts, locked_until, created_at, updated_at
           FROM users WHERE lower(email) = lower($1)"#,
    )
    .bind(email)
    .fetch_optional(db)
    .await
}

pub async fn find_user_by_id(db: &Db, id: Uuid) -> Result<Option<UserRow>, sqlx::Error> {
    sqlx::query_as::<_, UserRow>(
        r#"SELECT id, email, password_hash, name, email_verified_at, disabled_at,
                  failed_login_attempts, locked_until, created_at, updated_at
           FROM users WHERE id = $1"#,
    )
    .bind(id)
    .fetch_optional(db)
    .await
}

pub async fn record_failed_login(db: &Db, user_id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"UPDATE users
           SET failed_login_attempts = failed_login_attempts + 1,
               locked_until = CASE WHEN failed_login_attempts + 1 >= 5
                                   THEN now() + interval '15 minutes' ELSE locked_until END,
               updated_at = now()
           WHERE id = $1"#,
    )
    .bind(user_id)
    .execute(db)
    .await?;
    Ok(())
}

pub async fn clear_login_failures(db: &Db, user_id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE users SET failed_login_attempts = 0, locked_until = NULL, updated_at = now() WHERE id = $1",
    )
    .bind(user_id)
    .execute(db)
    .await?;
    Ok(())
}

pub async fn create_session(
    db: &Db,
    user_id: Uuid,
    token_hash: &str,
    ip: Option<&str>,
    user_agent: Option<&str>,
    expires_at: DateTime<Utc>,
) -> Result<Uuid, sqlx::Error> {
    let (id,): (Uuid,) = sqlx::query_as(
        r#"INSERT INTO sessions (user_id, token_hash, ip, user_agent, expires_at)
           VALUES ($1, $2, $3, $4, $5) RETURNING id"#,
    )
    .bind(user_id)
    .bind(token_hash)
    .bind(ip)
    .bind(user_agent.map(|s| s.chars().take(300).collect::<String>()))
    .bind(expires_at)
    .fetch_one(db)
    .await?;
    Ok(id)
}

pub async fn find_session_user(
    db: &Db,
    token_hash: &str,
) -> Result<Option<SessionUserRow>, sqlx::Error> {
    sqlx::query_as::<_, SessionUserRow>(
        r#"SELECT s.id AS session_id, u.id AS user_id, u.email, u.name,
                  s.expires_at AS session_expires_at, u.created_at AS user_created_at
           FROM sessions s
           JOIN users u ON u.id = s.user_id
           WHERE s.token_hash = $1
             AND s.revoked_at IS NULL
             AND s.expires_at > now()
             AND u.disabled_at IS NULL"#,
    )
    .bind(token_hash)
    .fetch_optional(db)
    .await
}

pub async fn touch_session(db: &Db, session_id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE sessions SET last_seen_at = now() WHERE id = $1")
        .bind(session_id)
        .execute(db)
        .await?;
    Ok(())
}

pub async fn revoke_session(db: &Db, session_id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE sessions SET revoked_at = now() WHERE id = $1")
        .bind(session_id)
        .execute(db)
        .await?;
    Ok(())
}

pub async fn revoke_all_sessions(db: &Db, user_id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE sessions SET revoked_at = now() WHERE user_id = $1 AND revoked_at IS NULL")
        .bind(user_id)
        .execute(db)
        .await?;
    Ok(())
}

pub async fn purge_expired_sessions(db: &Db) -> Result<u64, sqlx::Error> {
    let result = sqlx::query("DELETE FROM sessions WHERE expires_at < now() - interval '7 days'")
        .execute(db)
        .await?;
    Ok(result.rows_affected())
}

pub async fn memberships_for_user(
    db: &Db,
    user_id: Uuid,
) -> Result<Vec<MembershipContext>, sqlx::Error> {
    sqlx::query_as::<_, MembershipContext>(
        r#"SELECT o.id AS organization_id, o.name AS organization_name, o.slug AS organization_slug,
                  m.role, o.country, o.base_currency, o.is_demo, o.settings
           FROM organization_members m
           JOIN organizations o ON o.id = m.organization_id
           WHERE m.user_id = $1 AND o.deleted_at IS NULL
           ORDER BY o.created_at"#,
    )
    .bind(user_id)
    .fetch_all(db)
    .await
}

pub async fn membership_for_org(
    db: &Db,
    user_id: Uuid,
    organization_id: Uuid,
) -> Result<Option<MembershipContext>, sqlx::Error> {
    sqlx::query_as::<_, MembershipContext>(
        r#"SELECT o.id AS organization_id, o.name AS organization_name, o.slug AS organization_slug,
                  m.role, o.country, o.base_currency, o.is_demo, o.settings
           FROM organization_members m
           JOIN organizations o ON o.id = m.organization_id
           WHERE m.user_id = $1 AND o.id = $2 AND o.deleted_at IS NULL"#,
    )
    .bind(user_id)
    .bind(organization_id)
    .fetch_optional(db)
    .await
}

pub async fn is_organization_member(
    db: &Db,
    organization_id: Uuid,
    user_id: Uuid,
) -> Result<bool, sqlx::Error> {
    let found: Option<(i32,)> = sqlx::query_as(
        "SELECT 1 FROM organization_members WHERE organization_id = $1 AND user_id = $2",
    )
    .bind(organization_id)
    .bind(user_id)
    .fetch_optional(db)
    .await?;
    Ok(found.is_some())
}

pub async fn organization_by_id(
    db: &Db,
    organization_id: Uuid,
) -> Result<Option<OrganizationRow>, sqlx::Error> {
    sqlx::query_as::<_, OrganizationRow>(
        r#"SELECT id, name, slug, country, base_currency, settings, is_demo, created_at, updated_at
           FROM organizations WHERE id = $1 AND deleted_at IS NULL"#,
    )
    .bind(organization_id)
    .fetch_optional(db)
    .await
}

pub async fn update_organization(
    db: &Db,
    organization_id: Uuid,
    name: &str,
    country: &str,
    base_currency: &str,
    settings: &Value,
) -> Result<OrganizationRow, DbError> {
    let row = sqlx::query_as::<_, OrganizationRow>(
        r#"UPDATE organizations
           SET name = $2, country = $3, base_currency = $4, settings = $5, updated_at = now()
           WHERE id = $1 AND deleted_at IS NULL
           RETURNING id, name, slug, country, base_currency, settings, is_demo, created_at, updated_at"#,
    )
    .bind(organization_id)
    .bind(name)
    .bind(country)
    .bind(base_currency)
    .bind(settings)
    .fetch_optional(db)
    .await?;
    row.ok_or(DbError::NotFound)
}

pub async fn list_members(db: &Db, organization_id: Uuid) -> Result<Vec<MemberRow>, sqlx::Error> {
    sqlx::query_as::<_, MemberRow>(
        r#"SELECT u.id AS user_id, u.email, u.name, m.role, m.created_at,
                  (SELECT max(s.last_seen_at) FROM sessions s WHERE s.user_id = u.id AND s.revoked_at IS NULL) AS last_seen_at
           FROM organization_members m
           JOIN users u ON u.id = m.user_id
           WHERE m.organization_id = $1
           ORDER BY m.created_at"#,
    )
    .bind(organization_id)
    .fetch_all(db)
    .await
}

pub async fn update_member_role(
    db: &Db,
    organization_id: Uuid,
    user_id: Uuid,
    role: &str,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE organization_members SET role = $3, updated_at = now() WHERE organization_id = $1 AND user_id = $2",
    )
    .bind(organization_id)
    .bind(user_id)
    .bind(role)
    .execute(db)
    .await?;
    Ok(result.rows_affected() == 1)
}

pub async fn remove_member(
    db: &Db,
    organization_id: Uuid,
    user_id: Uuid,
) -> Result<bool, sqlx::Error> {
    let result =
        sqlx::query("DELETE FROM organization_members WHERE organization_id = $1 AND user_id = $2")
            .bind(organization_id)
            .bind(user_id)
            .execute(db)
            .await?;
    Ok(result.rows_affected() == 1)
}

pub async fn count_owners(db: &Db, organization_id: Uuid) -> Result<i64, sqlx::Error> {
    let (count,): (i64,) = sqlx::query_as(
        "SELECT count(*) FROM organization_members WHERE organization_id = $1 AND role = 'owner'",
    )
    .bind(organization_id)
    .fetch_one(db)
    .await?;
    Ok(count)
}

pub async fn create_invitation(
    db: &Db,
    organization_id: Uuid,
    email: &str,
    role: &str,
    token_hash: &str,
    invited_by: Uuid,
    expires_at: DateTime<Utc>,
) -> Result<Uuid, sqlx::Error> {
    let (id,): (Uuid,) = sqlx::query_as(
        r#"INSERT INTO invitations (organization_id, email, role, token_hash, invited_by, expires_at)
           VALUES ($1, lower($2), $3, $4, $5, $6) RETURNING id"#,
    )
    .bind(organization_id)
    .bind(email)
    .bind(role)
    .bind(token_hash)
    .bind(invited_by)
    .bind(expires_at)
    .fetch_one(db)
    .await?;
    Ok(id)
}

pub async fn list_invitations(
    db: &Db,
    organization_id: Uuid,
) -> Result<Vec<InvitationRow>, sqlx::Error> {
    sqlx::query_as::<_, InvitationRow>(
        r#"SELECT i.id, i.organization_id, i.email, i.role, u.name AS invited_by_name,
                  i.expires_at, i.accepted_at, i.created_at
           FROM invitations i JOIN users u ON u.id = i.invited_by
           WHERE i.organization_id = $1
           ORDER BY i.created_at DESC"#,
    )
    .bind(organization_id)
    .fetch_all(db)
    .await
}

pub async fn revoke_invitation(
    db: &Db,
    organization_id: Uuid,
    invitation_id: Uuid,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "DELETE FROM invitations WHERE id = $1 AND organization_id = $2 AND accepted_at IS NULL",
    )
    .bind(invitation_id)
    .bind(organization_id)
    .execute(db)
    .await?;
    Ok(result.rows_affected() == 1)
}

#[derive(Debug, Clone, FromRow)]
pub struct InvitationAcceptRow {
    pub id: Uuid,
    pub organization_id: Uuid,
    pub email: String,
    pub role: String,
    pub expires_at: DateTime<Utc>,
    pub accepted_at: Option<DateTime<Utc>>,
}

pub async fn find_invitation_by_hash(
    db: &Db,
    token_hash: &str,
) -> Result<Option<InvitationAcceptRow>, sqlx::Error> {
    sqlx::query_as::<_, InvitationAcceptRow>(
        "SELECT id, organization_id, email, role, expires_at, accepted_at FROM invitations WHERE token_hash = $1",
    )
    .bind(token_hash)
    .fetch_optional(db)
    .await
}

pub async fn accept_invitation(db: &Db, invitation_id: Uuid, user_id: Uuid) -> Result<(), DbError> {
    let mut tx = db.begin().await?;
    let row: Option<InvitationAcceptRow> = sqlx::query_as(
        "SELECT id, organization_id, email, role, expires_at, accepted_at FROM invitations WHERE id = $1 FOR UPDATE",
    )
    .bind(invitation_id)
    .fetch_optional(&mut *tx)
    .await?;
    let Some(row) = row else {
        return Err(DbError::NotFound);
    };
    if row.accepted_at.is_some() {
        return Err(DbError::Conflict("invitation already accepted".into()));
    }
    if row.expires_at < Utc::now() {
        return Err(DbError::Validation("invitation has expired".into()));
    }
    sqlx::query(
        r#"INSERT INTO organization_members (organization_id, user_id, role)
           VALUES ($1, $2, $3)
           ON CONFLICT (organization_id, user_id) DO UPDATE SET role = EXCLUDED.role, updated_at = now()"#,
    )
    .bind(row.organization_id)
    .bind(user_id)
    .bind(&row.role)
    .execute(&mut *tx)
    .await?;
    sqlx::query("UPDATE invitations SET accepted_at = now(), accepted_by = $2 WHERE id = $1")
        .bind(invitation_id)
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}

pub async fn create_password_reset(
    db: &Db,
    user_id: Uuid,
    token_hash: &str,
    expires_at: DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO password_reset_tokens (user_id, token_hash, expires_at) VALUES ($1, $2, $3)",
    )
    .bind(user_id)
    .bind(token_hash)
    .bind(expires_at)
    .execute(db)
    .await?;
    Ok(())
}

pub async fn consume_password_reset(
    db: &Db,
    token_hash: &str,
    new_password_hash: &str,
) -> Result<Uuid, DbError> {
    let mut tx = db.begin().await?;
    let row: Option<(Uuid, Uuid)> = sqlx::query_as(
        "SELECT id, user_id FROM password_reset_tokens WHERE token_hash = $1 AND used_at IS NULL AND expires_at > now() FOR UPDATE",
    )
    .bind(token_hash)
    .fetch_optional(&mut *tx)
    .await?;
    let Some((token_id, user_id)) = row else {
        return Err(DbError::Validation(
            "reset token is invalid or expired".into(),
        ));
    };
    sqlx::query("UPDATE users SET password_hash = $2, failed_login_attempts = 0, locked_until = NULL, updated_at = now() WHERE id = $1")
        .bind(user_id)
        .bind(new_password_hash)
        .execute(&mut *tx)
        .await?;
    sqlx::query("UPDATE password_reset_tokens SET used_at = now() WHERE id = $1")
        .bind(token_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("UPDATE sessions SET revoked_at = now() WHERE user_id = $1 AND revoked_at IS NULL")
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(user_id)
}

pub async fn update_password(
    db: &Db,
    user_id: Uuid,
    password_hash: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE users SET password_hash = $2, updated_at = now() WHERE id = $1")
        .bind(user_id)
        .bind(password_hash)
        .execute(db)
        .await?;
    Ok(())
}

pub async fn organization_settings(db: &Db, org_id: Uuid) -> Result<Value, DbError> {
    let row: Option<(Value,)> =
        sqlx::query_as("SELECT settings FROM organizations WHERE id = $1 AND deleted_at IS NULL")
            .bind(org_id)
            .fetch_optional(db)
            .await?;
    row.map(|r| r.0).ok_or(DbError::NotFound)
}
