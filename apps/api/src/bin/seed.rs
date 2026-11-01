use invoxa_api::config::{AppConfig, Environment};
use invoxa_api::state::initialize_tracing;
use invoxa_database::{auth, vendors};
use invoxa_security::hash_password_async;
use tracing::{info, warn};

const DEMO_EMAIL: &str = "demo@invoxa.local";
const DEMO_ORGANIZATION: &str = "Northstar Demo Company";
const DEMO_NAME: &str = "Demo Owner";
const DEFAULT_DEMO_PASSWORD: &str = "InvoxaDemo!2026";

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    initialize_tracing();

    let config = AppConfig::from_env()?;
    if config.environment == Environment::Production
        && std::env::var("SEED_ALLOW_PRODUCTION").as_deref() != Ok("true")
    {
        anyhow::bail!(
            "refusing to seed a production database; set SEED_ALLOW_PRODUCTION=true only if you intend to create a demo account"
        );
    }

    let password = std::env::var("SEED_DEMO_PASSWORD")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_DEMO_PASSWORD.to_string());
    if password.len() < 12 {
        anyhow::bail!("SEED_DEMO_PASSWORD must be at least 12 characters");
    }
    if config.environment == Environment::Production && password == DEFAULT_DEMO_PASSWORD {
        anyhow::bail!("set a unique SEED_DEMO_PASSWORD before seeding production");
    }

    let db =
        invoxa_database::connect(&config.database_url, config.database_max_connections).await?;
    invoxa_database::migrate(&db).await?;

    if let Some(existing) = auth::find_user_by_email(&db, DEMO_EMAIL).await? {
        let memberships = auth::memberships_for_user(&db, existing.id).await?;
        println!("Invoxa demo account already exists: {}", existing.email);
        for membership in memberships {
            println!(
                "  organization: {} ({})",
                membership.organization_name, membership.organization_id
            );
        }
        println!("No password was changed. Use the existing credentials or reset the password.");
        return Ok(());
    }

    let password_hash = hash_password_async(password.clone())
        .await
        .map_err(|error| anyhow::anyhow!("could not hash demo password: {error}"))?;
    let (user, organization) = auth::register_user_and_org(
        &db,
        DEMO_EMAIL,
        &password_hash,
        DEMO_NAME,
        DEMO_ORGANIZATION,
        "IN",
        "INR",
        true,
    )
    .await
    .map_err(|error| anyhow::anyhow!("could not create demo organization: {error}"))?;

    let (existing_vendors, _) = vendors::list_vendors(&db, organization.id, None, 1, 0).await?;
    if existing_vendors.is_empty() {
        let samples = [
            (
                "Acme Cloud Services Pvt Ltd",
                Some("29AABCA1234F1Z5"),
                Some("billing@acme-cloud.example"),
            ),
            (
                "Paper Trail Office Supplies",
                Some("27AACCP5678Q1Z2"),
                Some("accounts@papertrail.example"),
            ),
            (
                "Swift Logistics India",
                None,
                Some("invoices@swiftlogistics.example"),
            ),
        ];
        for (name, tax_id, email) in samples {
            match vendors::create_vendor(&db, organization.id, name, tax_id, email, None, None)
                .await
            {
                Ok(_) => {}
                Err(error) => {
                    warn!(vendor = name, error = %error, "could not create sample vendor")
                }
            }
        }
    }

    info!(
        user_id = %user.id,
        organization_id = %organization.id,
        "demo data seeded"
    );
    println!();
    println!("Invoxa demo workspace is ready.");
    println!("  Email:        {DEMO_EMAIL}");
    println!("  Password:     {password}");
    println!("  Organization: {}", organization.name);
    println!("  Org id:       {}", organization.id);
    println!();
    println!("Sign in at {}/login", config.app_url);
    println!("Change this password immediately if this workspace is reachable by others.");
    println!(
        "The demo organization is marked is_demo=true. AI_PROVIDER={} is labelled in every extracted invoice.",
        config.ai.kind.name()
    );
    Ok(())
}
