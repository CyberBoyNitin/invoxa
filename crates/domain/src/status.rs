use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DocumentStatus {
    Uploaded,
    Queued,
    ExtractingText,
    RunningOcr,
    ExtractingFields,
    Validating,
    ReviewRequired,
    ReadyForApproval,
    Approved,
    Exported,
    Rejected,
    Failed,
    Cancelled,
}

impl DocumentStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Uploaded => "uploaded",
            Self::Queued => "queued",
            Self::ExtractingText => "extracting_text",
            Self::RunningOcr => "running_ocr",
            Self::ExtractingFields => "extracting_fields",
            Self::Validating => "validating",
            Self::ReviewRequired => "review_required",
            Self::ReadyForApproval => "ready_for_approval",
            Self::Approved => "approved",
            Self::Exported => "exported",
            Self::Rejected => "rejected",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "uploaded" => Self::Uploaded,
            "queued" => Self::Queued,
            "extracting_text" => Self::ExtractingText,
            "running_ocr" => Self::RunningOcr,
            "extracting_fields" => Self::ExtractingFields,
            "validating" => Self::Validating,
            "review_required" => Self::ReviewRequired,
            "ready_for_approval" => Self::ReadyForApproval,
            "approved" => Self::Approved,
            "exported" => Self::Exported,
            "rejected" => Self::Rejected,
            "failed" => Self::Failed,
            "cancelled" => Self::Cancelled,
            _ => return None,
        })
    }

    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Approved | Self::Exported | Self::Rejected | Self::Failed | Self::Cancelled
        )
    }

    pub fn is_processing(self) -> bool {
        matches!(
            self,
            Self::Uploaded
                | Self::Queued
                | Self::ExtractingText
                | Self::RunningOcr
                | Self::ExtractingFields
                | Self::Validating
        )
    }

    /// Explicit document lifecycle. Processing stages are managed by the
    /// pipeline; review/approval transitions are enforced by the API.
    pub fn can_transition_to(self, next: Self) -> bool {
        use DocumentStatus::*;
        if self == next {
            return true;
        }
        match self {
            Uploaded => matches!(next, Queued | Failed | Cancelled),
            Queued => matches!(next, ExtractingText | Failed | Cancelled),
            ExtractingText => matches!(next, RunningOcr | ExtractingFields | Failed | Cancelled),
            RunningOcr => matches!(next, ExtractingFields | Failed | Cancelled),
            ExtractingFields => matches!(next, Validating | Failed | Cancelled),
            Validating => matches!(next, ReviewRequired | ReadyForApproval | Failed | Cancelled),
            ReviewRequired => matches!(
                next,
                ReadyForApproval | Approved | Rejected | Queued | Failed | Cancelled
            ),
            ReadyForApproval => {
                matches!(next, ReviewRequired | Approved | Rejected | Queued | Failed)
            }
            Approved => matches!(next, Exported | Rejected),
            Rejected => matches!(next, ReadyForApproval | ReviewRequired),
            Failed => matches!(next, Queued | Rejected | Cancelled),
            Exported | Cancelled => false,
        }
    }
}

impl fmt::Display for DocumentStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InvoiceStatus {
    ReviewRequired,
    ReadyForApproval,
    Approved,
    Rejected,
    Exported,
}

impl InvoiceStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ReviewRequired => "review_required",
            Self::ReadyForApproval => "ready_for_approval",
            Self::Approved => "approved",
            Self::Rejected => "rejected",
            Self::Exported => "exported",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "review_required" => Self::ReviewRequired,
            "ready_for_approval" => Self::ReadyForApproval,
            "approved" => Self::Approved,
            "rejected" => Self::Rejected,
            "exported" => Self::Exported,
            _ => return None,
        })
    }
}

impl fmt::Display for InvoiceStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobStatus {
    Queued,
    Running,
    Retry,
    Succeeded,
    Dead,
    Cancelled,
}

impl JobStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Retry => "retry",
            Self::Succeeded => "succeeded",
            Self::Dead => "dead",
            Self::Cancelled => "cancelled",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn processing_chain_is_valid() {
        use DocumentStatus::*;
        let chain = [
            Uploaded,
            Queued,
            ExtractingText,
            ExtractingFields,
            Validating,
            ReviewRequired,
            Approved,
            Exported,
        ];
        for pair in chain.windows(2) {
            assert!(
                pair[0].can_transition_to(pair[1]),
                "{} -> {} should be valid",
                pair[0],
                pair[1]
            );
        }
    }

    #[test]
    fn cannot_skip_extraction() {
        assert!(!DocumentStatus::Queued.can_transition_to(DocumentStatus::Approved));
        assert!(!DocumentStatus::Exported.can_transition_to(DocumentStatus::Rejected));
    }

    #[test]
    fn rejected_can_be_returned_to_review() {
        assert!(DocumentStatus::Rejected.can_transition_to(DocumentStatus::ReviewRequired));
    }
}
