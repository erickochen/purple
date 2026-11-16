//! Provider configuration, label migration, validation messages and
//! region picker copy. Vault-related provider validation (role format)
//! lives here because it is surfaced by the provider form, not the
//! vault signing flow.

pub fn provider_removed(display_name: &str) -> String {
    format!(
        "Removed {} configuration. Synced hosts remain in your SSH config.",
        display_name
    )
}

pub fn label_invalid(reason: &str) -> String {
    format!("Invalid name: {}", reason)
}

pub const LABEL_MUST_DIFFER: &str = "The two names must be different.";

pub fn label_already_in_use(label: &str) -> String {
    format!(
        "A config named '{}' already exists for this provider.",
        label
    )
}

pub const LABEL_MIGRATION_FIELD_CURRENT: &str = " Name for your current config ";
pub const LABEL_MIGRATION_FIELD_NEW: &str = " Name for the new config ";

pub const EXPAND_TO_REMOVE_CONFIG: &str =
    "Expand the provider and pick a specific config to remove.";

pub fn provider_not_configured(display_name: &str) -> String {
    format!("{} is not configured. Nothing to remove.", display_name)
}

pub fn provider_configure_first(display_name: &str) -> String {
    format!("Configure {} first. Press Enter to set up.", display_name)
}

pub fn provider_saved_syncing(display_name: &str) -> String {
    format!("Saved {} configuration. Syncing...", display_name)
}

pub fn provider_saved(display_name: &str) -> String {
    format!("Saved {} configuration.", display_name)
}

pub fn no_stale_hosts_for(display_name: &str) -> String {
    format!("No stale hosts for {}.", display_name)
}

pub fn contains_control_chars(name: &str) -> String {
    format!("{} contains control characters.", name)
}

/// Row suffix in the AWS profile picker for a profile that reaches its account
/// by assuming a role.
pub const PROFILE_ASSUMES_ROLE: &str = "assumes a role";

/// Row suffixes for a profile purple will refuse at sync time. A row has a few
/// words, so each names the shape and nothing else.
pub const PROFILE_NOTE_SSO: &str = "Identity Center, not read";
pub const PROFILE_NOTE_WEB_IDENTITY: &str = "web identity token, not exchanged";
pub const PROFILE_NOTE_CREDENTIAL_PROCESS: &str = "credential_process, not run";
pub const PROFILE_NOTE_CREDENTIAL_SOURCE: &str = "credential_source, not read";
pub const PROFILE_NOTE_MFA: &str = "needs an MFA code";
pub const PROFILE_NOTE_CHAIN: &str = "source_profile chain does not end";
pub const PROFILE_NOTE_NO_SOURCE: &str = "role_arn without source_profile";
/// A `source_profile` naming a profile that is not in either file. On a picker
/// row the profile itself exists, so this can only be the link it points at.
pub const PROFILE_NOTE_MISSING_SOURCE: &str = "source_profile is not there";
pub const PROFILE_NOTE_UNREADABLE: &str = "cannot be read";
pub const PROFILE_NOTE_NO_KEYS: &str = "no key pair";

pub const TOKEN_FORMAT_AWS: &str = "Format: AccessKeyId:Secret[:SessionToken]";
/// Sync-time failure for an AWS config that carries no token and no profile
/// while the environment holds no credentials either.
pub const AWS_NO_CREDENTIALS: &str = "No AWS credentials. Set a token or a profile on the provider. Otherwise export AWS_ACCESS_KEY_ID and AWS_SECRET_ACCESS_KEY.";

/// A profile is set, so it takes priority over the token and the environment.
/// These two say why it did not work, instead of blaming the API token.
pub fn aws_credentials_file_unreadable(path: &str) -> String {
    format!(
        "Can't read {}. The provider is set to use a profile, so nothing else is tried.",
        path
    )
}

/// `sources` is where purple looked, which the AWS file variables can move,
/// so the message never sends the user to a file it did not read.
pub fn aws_profile_not_found(profile: &str, sources: &str, known: &str) -> String {
    if known.is_empty() {
        format!(
            "Profile '{}' is in neither {}. A profile takes priority, so the token and the environment are not tried.",
            profile, sources
        )
    } else {
        format!(
            "Profile '{}' is in neither {}. Found: {}.",
            profile, sources, known
        )
    }
}

/// The profile block exists but holds no key pair and names no role.
pub fn aws_profile_without_keys(profile: &str) -> String {
    format!(
        "Profile '{}' has no credentials. Give it aws_access_key_id and aws_secret_access_key, or a role_arn with a source_profile.",
        profile
    )
}

/// Half a key pair. Naming the missing key beats letting AWS answer
/// `SignatureDoesNotMatch` to a request signed with whatever was found.
pub fn aws_profile_partial_credentials(profile: &str, missing_key: &str) -> String {
    format!(
        "Profile '{}' is missing {}. A key pair is read from one file, so add it beside the key it belongs to.",
        profile, missing_key
    )
}

/// A `role_arn` with nothing to assume it from.
pub fn aws_role_without_source(profile: &str) -> String {
    format!(
        "Profile '{}' sets role_arn but no source_profile. Name the profile holding the keys that may assume the role.",
        profile
    )
}

/// `credential_source` points at an instance or container role purple cannot read.
pub fn aws_credential_source_unsupported(profile: &str) -> String {
    format!(
        "Profile '{}' uses credential_source, which purple does not read. Point source_profile at a profile with keys instead.",
        profile
    )
}

/// `credential_process` runs an external command, which purple does not.
pub fn aws_credential_process_unsupported(profile: &str) -> String {
    format!(
        "Profile '{}' uses credential_process, which purple does not run. Export the keys it prints, or use a profile with keys.",
        profile
    )
}

/// `web_identity_token_file` signs in with an OIDC token, which is how an EKS
/// pod role or a CI job reaches AWS. The role is assumed by that exchange
/// rather than from a key pair, so there is no source profile to name.
pub fn aws_web_identity_unsupported(profile: &str) -> String {
    format!(
        "Profile '{}' signs in with web_identity_token_file, which purple does not exchange for credentials. Run 'aws configure export-credentials --profile {}' and paste the keys.",
        profile, profile
    )
}

/// IAM Identity Center keeps its tokens in a cache purple does not read.
pub fn aws_sso_unsupported(profile: &str) -> String {
    format!(
        "Profile '{}' signs in through IAM Identity Center, which purple does not read. Run 'aws configure export-credentials' and paste the keys, or use a role_arn with a source_profile.",
        profile
    )
}

/// The role wants an MFA code, and a sync has no terminal to ask on.
pub fn aws_mfa_unsupported(profile: &str) -> String {
    format!(
        "Profile '{}' sets mfa_serial. Sync runs in the background and cannot ask for a code. Assume the role yourself and export the keys.",
        profile
    )
}

/// A source_profile chain that points back at itself.
pub fn aws_profile_loop(profile: &str, sources: &str) -> String {
    format!(
        "Profile '{}' reaches itself through source_profile. Break the cycle in {}.",
        profile, sources
    )
}

/// A source_profile chain longer than purple follows.
pub fn aws_profile_chain_too_deep(profile: &str, sources: &str) -> String {
    format!(
        "The source_profile chain through '{}' is too long to follow. Shorten it in {}.",
        profile, sources
    )
}

/// Progress line while assuming one or more roles before the first API call.
pub fn aws_assuming_roles(count: usize) -> String {
    if count == 1 {
        "Assuming role...".to_string()
    } else {
        format!("Assuming {} roles...", count)
    }
}

/// An AssumeRole call that the service refused.
pub fn aws_assume_role_failed(role_arn: &str, detail: &str) -> String {
    format!("Could not assume {}. {}", role_arn, detail)
}

/// An AssumeRole response purple could not read credentials out of.
pub fn aws_assume_role_unparsable(role_arn: &str) -> String {
    format!(
        "Assumed {} but the response carried no credentials.",
        role_arn
    )
}

/// Progress line while asking Session Manager which nodes are reachable.
pub fn aws_ssm_checking(region: &str) -> String {
    format!("Checking Session Manager nodes in {}...", region)
}

/// End a service's own sentence with a full stop when it carries none, so it
/// does not run into the one purple puts after it. AWS error messages arrive
/// without closing punctuation.
fn sentence(detail: &str) -> String {
    let detail = detail.trim();
    if detail.is_empty() || detail.ends_with(['.', '!', '?']) {
        detail.to_string()
    } else {
        format!("{}.", detail)
    }
}

/// Session Manager could not be asked which nodes are reachable. Names the
/// action the lookup needs, because an EC2 read-only policy does not carry it
/// and the credentials themselves are usually fine.
pub fn aws_ssm_lookup_failed(region: &str, detail: &str) -> String {
    let head = format!("Could not list Session Manager nodes in {}.", region);
    let tail = "That lookup needs ssm:DescribeInstanceInformation; set Session Manager to 'always' to skip it.";
    match sentence(detail) {
        detail if detail.is_empty() => format!("{} {}", head, tail),
        detail => format!("{} {} {}", head, detail, tail),
    }
}

/// One region that did not finish, for the summary error when none did.
pub fn aws_region_failed(region: &str, detail: &str) -> String {
    match sentence(detail) {
        detail if detail.is_empty() => format!("{} failed.", region),
        detail => format!("{}: {}", region, detail),
    }
}

/// The sync ended with no instance at all while regions were failing. Counts
/// the regions that actually failed rather than claiming all of them did, and
/// carries the first one's own reason, since "check your credentials" is wrong
/// for a missing IAM action or an unreachable endpoint.
pub fn aws_no_instances_after_failures(
    failed: usize,
    total: usize,
    reason: Option<&str>,
) -> String {
    let head = match (failed, total) {
        (_, 1) => "No instances: the region failed.".to_string(),
        (f, t) if f == t => format!("No instances: all {} regions failed.", t),
        (f, t) => format!("No instances: {} of {} regions failed.", f, t),
    };
    match reason.map(sentence).filter(|r| !r.is_empty()) {
        Some(reason) => format!("{} {}", head, reason),
        None => format!("{} Check your credentials and region configuration.", head),
    }
}

/// Session Manager is on but no profile is set. The proxy command can carry a
/// `--profile` and nothing else, so an inline key pair never reaches the
/// session even though purple itself signs its own calls with it.
pub const AWS_SSM_WITHOUT_PROFILE: &str = "Session Manager is on without a profile. The proxy command passes no credentials, so the aws CLI uses its own for the session.";

/// A profile name that cannot be written into an SSM proxy command safely.
pub fn aws_ssm_profile_unsafe(profile: &str) -> String {
    format!(
        "Profile '{}' has characters purple will not put in a ProxyCommand. Rename it to letters, digits, dots, dashes or underscores.",
        profile
    )
}
pub const URL_REQUIRED_PROXMOX: &str = "URL is required for Proxmox VE.";
pub const URL_REQUIRED_NETBOX: &str = "URL is required for NetBox.";
pub const SSH_PORT_INVALID: &str = "SSH port must be a number from 1 to 65535.";
pub const PROJECT_REQUIRED_GCP: &str = "Project ID can't be empty. Set your GCP project ID.";
pub const COMPARTMENT_REQUIRED_OCI: &str =
    "Compartment can't be empty. Set your OCI compartment OCID.";
pub const REGIONS_REQUIRED_AWS: &str = "Select at least one AWS region.";
pub const ZONES_REQUIRED_SCALEWAY: &str = "Select at least one Scaleway zone.";
pub const SUBSCRIPTIONS_REQUIRED_AZURE: &str = "Enter at least one Azure subscription ID.";
pub const ALIAS_PREFIX_INVALID: &str =
    "Alias prefix can't contain spaces or pattern characters (*, ?, [, !).";
pub const USER_NO_WHITESPACE: &str = "User can't contain whitespace.";
pub const VAULT_ROLE_FORMAT: &str = "Vault SSH role must be in the form <mount>/sign/<role>.";

pub const PROVIDER_CONFIG_CHANGED_EXTERNALLY: &str =
    "Provider config changed externally. Press Esc and re-open to pick up changes.";
pub const PROVIDER_URL_REQUIRES_HTTPS: &str =
    "URL must start with https://. Toggle Verify TLS off for self-signed certificates.";
pub const PROVIDER_TOKEN_REQUIRED_GCP: &str =
    "Token can't be empty. Provide a service account JSON key file path or access token.";
pub const PROVIDER_TOKEN_REQUIRED_ORACLE: &str =
    "Token can't be empty. Provide the path to your OCI config file (e.g. ~/.oci/config).";

pub fn provider_token_required(display_name: &str) -> String {
    format!(
        "Token can't be empty. Grab one from your {} dashboard.",
        display_name
    )
}

pub fn azure_subscription_id_invalid(sub: &str) -> String {
    format!(
        "Invalid subscription ID '{}'. Expected UUID format \
         (e.g. 12345678-1234-1234-1234-123456789012).",
        sub
    )
}

// ── Region picker ───────────────────────────────────────────────────

pub fn regions_selected_count(count: usize, label: &str) -> String {
    let s = if count == 1 { "" } else { "s" };
    format!("{} {}{} selected.", count, label, s)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A real AccessDeniedException body, which AWS sends without closing
    /// punctuation.
    const DENIED: &str = "AccessDeniedException: User: arn:aws:iam::1:user/eric is not authorized to perform: ssm:DescribeInstanceInformation on resource: *";

    #[test]
    fn a_service_sentence_gets_the_full_stop_it_arrived_without() {
        let rendered = aws_ssm_lookup_failed("eu-west-1", DENIED);
        assert!(
            rendered.contains("on resource: *. That lookup needs"),
            "the two sentences run together: {rendered}"
        );
    }

    #[test]
    fn a_service_sentence_that_already_ends_keeps_its_own_punctuation() {
        let rendered = aws_ssm_lookup_failed("eu-west-1", "Denied.");
        assert!(!rendered.contains("Denied.."), "{rendered}");
        assert!(rendered.contains("Denied. That lookup needs"), "{rendered}");
    }

    #[test]
    fn an_empty_detail_leaves_no_gap_behind() {
        for detail in ["", "   "] {
            let rendered = aws_ssm_lookup_failed("eu-west-1", detail);
            assert!(!rendered.contains("  "), "double space: {rendered}");
            assert!(
                rendered.contains("in eu-west-1. That lookup needs"),
                "{rendered}"
            );
        }
        assert_eq!(aws_region_failed("eu-west-1", ""), "eu-west-1 failed.");
    }

    #[test]
    fn the_region_summary_counts_only_the_regions_that_failed() {
        assert!(
            aws_no_instances_after_failures(1, 1, None).starts_with("No instances: the region"),
            "one configured region is not a plural"
        );
        assert!(aws_no_instances_after_failures(3, 3, None).contains("all 3 regions failed"),);
        // A region that returned nothing is not a region that failed.
        assert!(aws_no_instances_after_failures(1, 2, None).contains("1 of 2 regions failed"));
    }

    #[test]
    fn the_region_summary_falls_back_when_there_is_no_reason() {
        let rendered = aws_no_instances_after_failures(2, 2, Some("   "));
        assert!(rendered.contains("Check your credentials"), "{rendered}");
    }
}
