//! Per-site permission decisions, saved as JSON.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// What the user decided for one permission on one site.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Decision {
    /// Granted without asking again.
    Allow,
    /// Refused without asking again.
    Block,
}

/// Decisions keyed by origin, then by permission name (`"camera"`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SitePermissions {
    sites: BTreeMap<String, BTreeMap<String, Decision>>,
}

impl SitePermissions {
    /// The stored decision, if any.
    pub fn get(&self, origin: &str, permission: &str) -> Option<Decision> {
        self.sites.get(origin)?.get(permission).copied()
    }

    /// Stores a decision.
    pub fn set(&mut self, origin: &str, permission: &str, decision: Decision) {
        self.sites
            .entry(origin.to_string())
            .or_default()
            .insert(permission.to_string(), decision);
    }

    /// Forgets one decision, so the site asks again.
    pub fn revoke(&mut self, origin: &str, permission: &str) {
        if let Some(site) = self.sites.get_mut(origin) {
            site.remove(permission);
            if site.is_empty() {
                self.sites.remove(origin);
            }
        }
    }

    /// Forgets every decision.
    pub fn clear(&mut self) {
        self.sites.clear();
    }

    /// Sites with their decisions, sorted by origin.
    pub fn sites(&self) -> impl Iterator<Item = (&str, &BTreeMap<String, Decision>)> {
        self.sites
            .iter()
            .map(|(origin, decisions)| (origin.as_str(), decisions))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decisions_are_per_origin_and_permission() {
        let mut permissions = SitePermissions::default();
        permissions.set("https://a.com", "camera", Decision::Allow);
        permissions.set("https://a.com", "geolocation", Decision::Block);
        assert_eq!(
            permissions.get("https://a.com", "camera"),
            Some(Decision::Allow)
        );
        assert_eq!(permissions.get("https://b.com", "camera"), None);
        assert_eq!(permissions.get("https://a.com", "microphone"), None);
    }

    #[test]
    fn revoking_the_last_decision_drops_the_site() {
        let mut permissions = SitePermissions::default();
        permissions.set("https://a.com", "camera", Decision::Allow);
        permissions.revoke("https://a.com", "camera");
        assert_eq!(permissions.sites().count(), 0);
    }

    #[test]
    fn round_trips_through_json() {
        let mut permissions = SitePermissions::default();
        permissions.set("https://a.com", "notifications", Decision::Block);
        let json = serde_json::to_string(&permissions).unwrap();
        assert!(json.contains(r#""notifications":"block""#));
        assert_eq!(
            serde_json::from_str::<SitePermissions>(&json).unwrap(),
            permissions
        );
    }
}
