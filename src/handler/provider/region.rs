use crossterm::event::{KeyCode, KeyEvent};

use crate::app::{App, Screen};

type ZoneList = &'static [(&'static str, &'static str)];
type ZoneGroups = &'static [(&'static str, usize, usize)];

/// Build the same row list used by the region picker renderer.
pub(crate) fn region_picker_rows(provider: &str) -> Vec<Option<&'static str>> {
    let (zones, groups) = zone_data_for(provider);
    let mut rows = Vec::new();
    for &(_, start, end) in groups {
        rows.push(None); // group header
        for &(code, _) in &zones[start..end] {
            rows.push(Some(code));
        }
    }
    rows
}

/// Rebuild the regions string from the selected set, preserving display order.
pub(crate) fn rebuild_regions_string(
    selected: &std::collections::HashSet<String>,
    provider: &str,
) -> String {
    let (zones, _) = zone_data_for(provider);
    let ordered: Vec<&str> = zones
        .iter()
        .filter(|(code, _)| selected.contains(*code))
        .map(|(code, _)| *code)
        .collect();
    ordered.join(",")
}

/// Return the zone/region data for a provider (aws or scaleway).
pub(crate) fn zone_data_for(provider: &str) -> (ZoneList, ZoneGroups) {
    match provider {
        "scaleway" => (
            crate::providers::scaleway::SCW_ZONES,
            crate::providers::scaleway::SCW_ZONE_GROUPS,
        ),
        "aws" => (
            crate::providers::aws::AWS_REGIONS,
            crate::providers::aws::AWS_REGION_GROUPS,
        ),
        "gcp" => (
            crate::providers::gcp::GCP_ZONES,
            crate::providers::gcp::GCP_ZONE_GROUPS,
        ),
        "oracle" => (
            crate::providers::oracle::OCI_REGIONS,
            crate::providers::oracle::OCI_REGION_GROUPS,
        ),
        "ovh" => (
            crate::providers::ovh::OVH_ENDPOINTS,
            crate::providers::ovh::OVH_ENDPOINT_GROUPS,
        ),
        _ => {
            debug_assert!(
                false,
                "zone_data_for called for unsupported provider: {provider}"
            );
            (&[], &[])
        }
    }
}

pub(crate) fn handle_region_picker(app: &mut App, key: KeyEvent) {
    let provider_name = match &app.screen {
        Screen::ProviderForm { provider } => provider.clone(),
        _ => return,
    };
    let rows = region_picker_rows(&provider_name);
    let total = rows.len();

    // Parse current regions into a set for toggling
    let mut selected: std::collections::HashSet<String> = app
        .provider_form
        .regions
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    let zone_label = if matches!(provider_name.as_str(), "scaleway" | "gcp") {
        "zone"
    } else if provider_name == "ovh" {
        "endpoint"
    } else {
        "region"
    };

    match key.code {
        KeyCode::Esc => {
            app.provider_form.regions = rebuild_regions_string(&selected, &provider_name);
            app.provider_form.sync_cursor_to_end();
            app.ui.show_region_picker = false;
            let count = selected.len();
            if count > 0 {
                app.set_status(
                    format!(
                        "{} {}{} selected.",
                        count,
                        zone_label,
                        if count == 1 { "" } else { "s" }
                    ),
                    false,
                );
            }
        }
        KeyCode::Enter => {
            // For single-select providers (OVH): Enter on an item selects it
            // exclusively and closes. For multi-select: Enter confirms current
            // selection (same as Esc).
            if provider_name == "ovh" {
                let cursor = app.ui.region_picker_cursor;
                if let Some(Some(code)) = rows.get(cursor) {
                    selected.clear();
                    selected.insert(code.to_string());
                }
            }
            app.provider_form.regions = rebuild_regions_string(&selected, &provider_name);
            app.provider_form.sync_cursor_to_end();
            app.ui.show_region_picker = false;
            let count = selected.len();
            if count > 0 {
                app.set_status(
                    format!(
                        "{} {}{} selected.",
                        count,
                        zone_label,
                        if count == 1 { "" } else { "s" }
                    ),
                    false,
                );
            }
        }
        KeyCode::Down | KeyCode::Char('j') => {
            if app.ui.region_picker_cursor + 1 < total {
                app.ui.region_picker_cursor += 1;
            }
        }
        KeyCode::Up | KeyCode::Char('k') => {
            if app.ui.region_picker_cursor > 0 {
                app.ui.region_picker_cursor -= 1;
            }
        }
        KeyCode::Char(' ') => {
            let cursor = app.ui.region_picker_cursor;
            if let Some(Some(code)) = rows.get(cursor) {
                // Toggle single region
                if selected.contains(*code) {
                    selected.remove(*code);
                } else {
                    selected.insert(code.to_string());
                }
            } else {
                // Group header: toggle all regions in this group
                let group_codes: Vec<&str> = rows[cursor + 1..]
                    .iter()
                    .take_while(|r| r.is_some())
                    .filter_map(|r| *r)
                    .collect();
                let all_selected = group_codes.iter().all(|c| selected.contains(*c));
                for code in group_codes {
                    if all_selected {
                        selected.remove(code);
                    } else {
                        selected.insert(code.to_string());
                    }
                }
            }
            app.provider_form.regions = rebuild_regions_string(&selected, &provider_name);
        }
        _ => {}
    }
}
