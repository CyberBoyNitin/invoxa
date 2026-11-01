use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Owner,
    Admin,
    Accountant,
    Reviewer,
    ReadOnly,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Owner => "owner",
            Self::Admin => "admin",
            Self::Accountant => "accountant",
            Self::Reviewer => "reviewer",
            Self::ReadOnly => "read_only",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "owner" => Some(Self::Owner),
            "admin" => Some(Self::Admin),
            "accountant" => Some(Self::Accountant),
            "reviewer" => Some(Self::Reviewer),
            "read_only" => Some(Self::ReadOnly),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Owner => "Owner",
            Self::Admin => "Admin",
            Self::Accountant => "Accountant",
            Self::Reviewer => "Reviewer",
            Self::ReadOnly => "Read-only",
        }
    }
}

impl fmt::Display for Role {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Permission {
    ViewDocuments,
    UploadDocuments,
    DeleteDocuments,
    EditInvoice,
    ApproveInvoice,
    RejectInvoice,
    ExportInvoices,
    ManageVendors,
    ManageAutomations,
    ManageApiKeys,
    ManageWebhooks,
    ManageIntegrations,
    ManageMembers,
    ManageOrganization,
    ViewAuditLog,
}

impl Permission {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ViewDocuments => "documents:read",
            Self::UploadDocuments => "documents:write",
            Self::DeleteDocuments => "documents:delete",
            Self::EditInvoice => "invoices:write",
            Self::ApproveInvoice => "invoices:approve",
            Self::RejectInvoice => "invoices:reject",
            Self::ExportInvoices => "exports:write",
            Self::ManageVendors => "vendors:write",
            Self::ManageAutomations => "automations:write",
            Self::ManageApiKeys => "api_keys:write",
            Self::ManageWebhooks => "webhooks:write",
            Self::ManageIntegrations => "integrations:write",
            Self::ManageMembers => "members:write",
            Self::ManageOrganization => "organization:write",
            Self::ViewAuditLog => "audit:read",
        }
    }
}

impl Role {
    /// Server-side permission matrix. UI visibility is not a security control.
    pub fn can(self, permission: Permission) -> bool {
        use Permission::*;
        match self {
            Role::Owner => true,
            Role::Admin => matches!(
                permission,
                ViewDocuments
                    | UploadDocuments
                    | DeleteDocuments
                    | EditInvoice
                    | ApproveInvoice
                    | RejectInvoice
                    | ExportInvoices
                    | ManageVendors
                    | ManageAutomations
                    | ManageApiKeys
                    | ManageWebhooks
                    | ManageIntegrations
                    | ViewAuditLog
            ),
            Role::Accountant => matches!(
                permission,
                ViewDocuments
                    | UploadDocuments
                    | EditInvoice
                    | ApproveInvoice
                    | RejectInvoice
                    | ExportInvoices
                    | ManageVendors
            ),
            Role::Reviewer => matches!(permission, ViewDocuments | EditInvoice),
            Role::ReadOnly => matches!(permission, ViewDocuments),
        }
    }

    pub fn rank(self) -> u8 {
        match self {
            Role::Owner => 5,
            Role::Admin => 4,
            Role::Accountant => 3,
            Role::Reviewer => 2,
            Role::ReadOnly => 1,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_only_cannot_write() {
        assert!(!Role::ReadOnly.can(Permission::EditInvoice));
        assert!(!Role::ReadOnly.can(Permission::ApproveInvoice));
        assert!(Role::ReadOnly.can(Permission::ViewDocuments));
    }

    #[test]
    fn reviewer_can_correct_but_not_approve() {
        assert!(Role::Reviewer.can(Permission::EditInvoice));
        assert!(!Role::Reviewer.can(Permission::ApproveInvoice));
        assert!(!Role::Reviewer.can(Permission::ExportInvoices));
    }

    #[test]
    fn admin_has_operational_permissions_but_not_members() {
        assert!(Role::Admin.can(Permission::ManageApiKeys));
        assert!(!Role::Admin.can(Permission::ManageMembers));
        assert!(!Role::Admin.can(Permission::ManageOrganization));
    }

    #[test]
    fn owner_has_everything() {
        for p in [
            Permission::ViewDocuments,
            Permission::ApproveInvoice,
            Permission::ManageMembers,
            Permission::ManageOrganization,
        ] {
            assert!(Role::Owner.can(p));
        }
    }
}
