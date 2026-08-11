//! Kubernetes API discovery surface.
//!
//! `kubectl` issues its discovery requests *before* anything else and gives up with an
//! unhelpful error if they are malformed, so these documents are built deterministically by
//! NetGet rather than asked of the model on every request. They are protocol envelope — the
//! shape of the API — not cluster content. What resources exist inside the cluster (pods,
//! nodes, CRs) is still entirely the model's to invent, via the `k8s_*` actions.
//!
//! The advertised resource set itself *is* model-controlled: the `resources` startup parameter
//! replaces the default table wholesale, which is how a caller advertises CRDs.

use anyhow::{anyhow, Result};
use serde_json::{json, Value};

/// One entry in the API discovery table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApiResource {
    /// API group; empty string for the core (legacy) group served under `/api`.
    pub group: String,
    /// Group version, e.g. `v1`.
    pub version: String,
    /// Plural, lowercase resource name as it appears in the URL path, e.g. `pods`.
    pub name: String,
    /// Object kind, e.g. `Pod`.
    pub kind: String,
    /// Whether the resource lives under `/namespaces/{ns}/`.
    pub namespaced: bool,
    /// Short names accepted by kubectl, e.g. `po`.
    pub short_names: Vec<String>,
    /// Verbs advertised for this resource.
    pub verbs: Vec<String>,
}

impl ApiResource {
    fn new(
        group: &str,
        version: &str,
        name: &str,
        kind: &str,
        namespaced: bool,
        short_names: &[&str],
    ) -> Self {
        Self {
            group: group.to_string(),
            version: version.to_string(),
            name: name.to_string(),
            kind: kind.to_string(),
            namespaced,
            short_names: short_names.iter().map(|s| s.to_string()).collect(),
            verbs: default_verbs(),
        }
    }

    /// `v1` for the core group, `apps/v1` otherwise — the value that goes in `apiVersion`.
    pub fn group_version(&self) -> String {
        if self.group.is_empty() {
            self.version.clone()
        } else {
            format!("{}/{}", self.group, self.version)
        }
    }

    fn to_json(&self) -> Value {
        let mut entry = json!({
            "name": self.name,
            "singularName": "",
            "namespaced": self.namespaced,
            "kind": self.kind,
            "verbs": self.verbs,
        });
        if !self.short_names.is_empty() {
            entry["shortNames"] = json!(self.short_names);
        }
        entry
    }
}

fn default_verbs() -> Vec<String> {
    [
        "create",
        "delete",
        "deletecollection",
        "get",
        "list",
        "patch",
        "update",
        "watch",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

/// The full advertised API surface of one server instance.
#[derive(Clone, Debug)]
pub struct ApiSurface {
    resources: Vec<ApiResource>,
}

impl Default for ApiSurface {
    fn default() -> Self {
        Self::builtin()
    }
}

impl ApiSurface {
    /// The default surface: core/v1 plus the handful of groups kubectl users reach for first.
    pub fn builtin() -> Self {
        let resources = vec![
            ApiResource::new("", "v1", "pods", "Pod", true, &["po"]),
            ApiResource::new("", "v1", "services", "Service", true, &["svc"]),
            ApiResource::new("", "v1", "nodes", "Node", false, &["no"]),
            ApiResource::new("", "v1", "namespaces", "Namespace", false, &["ns"]),
            ApiResource::new("", "v1", "configmaps", "ConfigMap", true, &["cm"]),
            ApiResource::new("", "v1", "secrets", "Secret", true, &[]),
            ApiResource::new("", "v1", "events", "Event", true, &["ev"]),
            ApiResource::new("", "v1", "endpoints", "Endpoints", true, &["ep"]),
            ApiResource::new("", "v1", "serviceaccounts", "ServiceAccount", true, &["sa"]),
            ApiResource::new(
                "",
                "v1",
                "persistentvolumeclaims",
                "PersistentVolumeClaim",
                true,
                &["pvc"],
            ),
            ApiResource::new(
                "",
                "v1",
                "persistentvolumes",
                "PersistentVolume",
                false,
                &["pv"],
            ),
            ApiResource::new(
                "",
                "v1",
                "replicationcontrollers",
                "ReplicationController",
                true,
                &["rc"],
            ),
            ApiResource::new("apps", "v1", "deployments", "Deployment", true, &["deploy"]),
            ApiResource::new("apps", "v1", "replicasets", "ReplicaSet", true, &["rs"]),
            ApiResource::new("apps", "v1", "statefulsets", "StatefulSet", true, &["sts"]),
            ApiResource::new("apps", "v1", "daemonsets", "DaemonSet", true, &["ds"]),
            ApiResource::new("batch", "v1", "jobs", "Job", true, &[]),
            ApiResource::new("batch", "v1", "cronjobs", "CronJob", true, &["cj"]),
            ApiResource::new(
                "apiextensions.k8s.io",
                "v1",
                "customresourcedefinitions",
                "CustomResourceDefinition",
                false,
                &["crd", "crds"],
            ),
        ];
        Self { resources }
    }

    /// Build a surface from the `resources` startup parameter.
    ///
    /// Every entry must carry `name` and `kind`; everything else has a sane default. A
    /// malformed entry is an error rather than a silent skip, so a caller that mistypes a CRD
    /// gets `ServerStatus::Error` instead of a cluster that is quietly missing it.
    pub fn from_startup_value(values: &[Value]) -> Result<Self> {
        if values.is_empty() {
            return Err(anyhow!(
                "startup parameter 'resources' was an empty array; omit it to use the built-in \
                 Kubernetes API surface"
            ));
        }
        let mut resources = Vec::with_capacity(values.len());
        for (idx, value) in values.iter().enumerate() {
            let obj = value
                .as_object()
                .ok_or_else(|| anyhow!("startup parameter 'resources'[{idx}] must be an object"))?;
            let name = obj
                .get("name")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow!("'resources'[{idx}] is missing the required string 'name' (the plural URL segment, e.g. \"pods\")"))?;
            let kind = obj.get("kind").and_then(Value::as_str).ok_or_else(|| {
                anyhow!("'resources'[{idx}] is missing the required string 'kind' (e.g. \"Pod\")")
            })?;
            let group = obj.get("group").and_then(Value::as_str).unwrap_or("");
            let version = obj.get("version").and_then(Value::as_str).unwrap_or("v1");
            let namespaced = obj
                .get("namespaced")
                .and_then(Value::as_bool)
                .unwrap_or(true);
            let short_names = obj
                .get("shortNames")
                .or_else(|| obj.get("short_names"))
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(Value::as_str)
                        .map(|s| s.to_string())
                        .collect()
                })
                .unwrap_or_default();
            let verbs = obj
                .get("verbs")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(Value::as_str)
                        .map(|s| s.to_string())
                        .collect::<Vec<_>>()
                })
                .filter(|v: &Vec<String>| !v.is_empty())
                .unwrap_or_else(default_verbs);

            resources.push(ApiResource {
                group: group.to_string(),
                version: version.to_string(),
                name: name.to_string(),
                kind: kind.to_string(),
                namespaced,
                short_names,
                verbs,
            });
        }
        Ok(Self { resources })
    }

    /// Look a resource up by its route coordinates.
    pub fn find(&self, group: &str, version: &str, name: &str) -> Option<&ApiResource> {
        self.resources
            .iter()
            .find(|r| r.group == group && r.version == version && r.name == name)
    }

    /// Core-group versions, in first-seen order. Serves `GET /api`.
    fn core_versions(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for r in self.resources.iter().filter(|r| r.group.is_empty()) {
            if !out.contains(&r.version) {
                out.push(r.version.clone());
            }
        }
        if out.is_empty() {
            out.push("v1".to_string());
        }
        out
    }

    /// Non-core (group, versions) pairs, in first-seen order. Serves `GET /apis`.
    fn named_groups(&self) -> Vec<(String, Vec<String>)> {
        let mut out: Vec<(String, Vec<String>)> = Vec::new();
        for r in self.resources.iter().filter(|r| !r.group.is_empty()) {
            match out.iter_mut().find(|(g, _)| *g == r.group) {
                Some((_, versions)) => {
                    if !versions.contains(&r.version) {
                        versions.push(r.version.clone());
                    }
                }
                None => out.push((r.group.clone(), vec![r.version.clone()])),
            }
        }
        out
    }

    /// `GET /api` → `APIVersions`.
    pub fn api_versions(&self, server_address: &str) -> Value {
        json!({
            "kind": "APIVersions",
            "versions": self.core_versions(),
            "serverAddressByClientCIDRs": [{
                "clientCIDR": "0.0.0.0/0",
                "serverAddress": server_address,
            }],
        })
    }

    /// `GET /apis` → `APIGroupList`.
    pub fn api_group_list(&self) -> Value {
        let groups: Vec<Value> = self
            .named_groups()
            .into_iter()
            .map(|(group, versions)| group_json(&group, &versions))
            .collect();
        json!({
            "kind": "APIGroupList",
            "apiVersion": "v1",
            "groups": groups,
        })
    }

    /// `GET /apis/{group}` → `APIGroup`, or `None` when the group is not advertised.
    pub fn api_group(&self, group: &str) -> Option<Value> {
        self.named_groups()
            .into_iter()
            .find(|(g, _)| g == group)
            .map(|(g, versions)| {
                let mut value = group_json(&g, &versions);
                value["kind"] = json!("APIGroup");
                value["apiVersion"] = json!("v1");
                value
            })
    }

    /// `GET /api/v1` or `GET /apis/{group}/{version}` → `APIResourceList`.
    ///
    /// Returns `None` when nothing is advertised at that group version, so the caller can
    /// answer with a proper 404 `Status` instead of an empty list.
    pub fn api_resource_list(&self, group: &str, version: &str) -> Option<Value> {
        let resources: Vec<Value> = self
            .resources
            .iter()
            .filter(|r| r.group == group && r.version == version)
            .map(ApiResource::to_json)
            .collect();
        if resources.is_empty() {
            return None;
        }
        let group_version = if group.is_empty() {
            version.to_string()
        } else {
            format!("{group}/{version}")
        };
        Some(json!({
            "kind": "APIResourceList",
            "apiVersion": "v1",
            "groupVersion": group_version,
            "resources": resources,
        }))
    }
}

fn group_json(group: &str, versions: &[String]) -> Value {
    let version_entries: Vec<Value> = versions
        .iter()
        .map(|v| {
            json!({
                "groupVersion": format!("{group}/{v}"),
                "version": v,
            })
        })
        .collect();
    let preferred = version_entries
        .first()
        .cloned()
        .unwrap_or_else(|| json!({"groupVersion": format!("{group}/v1"), "version": "v1"}));
    json!({
        "name": group,
        "versions": version_entries,
        "preferredVersion": preferred,
    })
}

/// `GET /version` → the `version.Info` struct `kubectl version` prints.
///
/// `version` is the `kubernetes_version` startup parameter, e.g. `v1.29.4`; major/minor are
/// derived from it so the three fields can never disagree.
pub fn version_info(version: &str) -> Value {
    let trimmed = version.trim_start_matches('v');
    let mut parts = trimmed.split('.');
    let major = parts.next().unwrap_or("1");
    let minor = parts.next().unwrap_or("29");
    let git_version = if version.starts_with('v') {
        version.to_string()
    } else {
        format!("v{version}")
    };
    json!({
        "major": major,
        "minor": minor,
        "gitVersion": git_version,
        "gitCommit": "0000000000000000000000000000000000000000",
        "gitTreeState": "clean",
        "buildDate": "1970-01-01T00:00:00Z",
        "goVersion": "go1.21.0",
        "compiler": "gc",
        "platform": "linux/amd64",
    })
}

/// Build a Kubernetes `Status` object — the error envelope every real client understands.
pub fn status_object(code: u16, reason: &str, message: &str, details: Option<Value>) -> Value {
    let mut value = json!({
        "kind": "Status",
        "apiVersion": "v1",
        "metadata": {},
        "status": if (200..300).contains(&code) { "Success" } else { "Failure" },
        "message": message,
        "reason": reason,
        "code": code,
    });
    if let Some(details) = details {
        value["details"] = details;
    }
    value
}
