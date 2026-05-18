use axum::{
    extract::State,
    http::Request,
    middleware::Next,
    response::{IntoResponse, Response},
};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::warn;

use crate::auth::Claims;

/// Resource types that can be protected
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub enum ResourceType {
    Devices,
    Models,
    Deployments,
    Users,
    AuditLogs,
    Alerts,
}

/// Action types on resources
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub enum Action {
    Read,
    Create,
    Update,
    Delete,
}

impl std::fmt::Display for Action {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Action::Read => write!(f, "READ"),
            Action::Create => write!(f, "CREATE"),
            Action::Update => write!(f, "UPDATE"),
            Action::Delete => write!(f, "DELETE"),
        }
    }
}

/// Role-based permissions
#[derive(Debug, Clone)]
pub struct RolePermissions {
    pub allowed_actions: HashSet<(ResourceType, Action)>,
}

impl RolePermissions {
    pub fn admin() -> Self {
        Self {
            allowed_actions: HashSet::new(), // Admin has all permissions, checked separately
        }
    }

    pub fn user() -> Self {
        let mut actions = HashSet::new();
        // Users can read and create devices, models, deployments
        actions.insert((ResourceType::Devices, Action::Read));
        actions.insert((ResourceType::Devices, Action::Create));
        actions.insert((ResourceType::Models, Action::Read));
        actions.insert((ResourceType::Models, Action::Create));
        actions.insert((ResourceType::Deployments, Action::Read));
        actions.insert((ResourceType::Deployments, Action::Create));
        actions.insert((ResourceType::Alerts, Action::Read));
        actions.insert((ResourceType::AuditLogs, Action::Read));
        Self {
            allowed_actions: actions,
        }
    }

    pub fn device_operator() -> Self {
        let mut actions = HashSet::new();
        // Device operators can only read devices and deployments
        actions.insert((ResourceType::Devices, Action::Read));
        actions.insert((ResourceType::Deployments, Action::Read));
        Self {
            allowed_actions: actions,
        }
    }
}

/// RBAC state for middleware
#[derive(Clone)]
pub struct RbacState {
    role_permissions: Arc<RwLock<HashMap<String, RolePermissions>>>,
}

impl RbacState {
    pub fn new() -> Self {
        let mut role_permissions = HashMap::new();
        role_permissions.insert("admin".to_string(), RolePermissions::admin());
        role_permissions.insert("user".to_string(), RolePermissions::user());
        role_permissions.insert(
            "device_operator".to_string(),
            RolePermissions::device_operator(),
        );

        Self {
            role_permissions: Arc::new(RwLock::new(role_permissions)),
        }
    }

    pub async fn check_permission(
        &self,
        role: &str,
        resource: ResourceType,
        action: Action,
    ) -> bool {
        let roles = self.role_permissions.read().await;

        // Admin has all permissions
        if role == "admin" {
            return true;
        }

        if let Some(role_perms) = roles.get(role) {
            return role_perms.allowed_actions.contains(&(resource, action));
        }

        false
    }

    pub async fn set_role_permissions(&self, role: String, permissions: RolePermissions) {
        let mut roles = self.role_permissions.write().await;
        roles.insert(role, permissions);
    }
}

impl Default for RbacState {
    fn default() -> Self {
        Self::new()
    }
}

/// Parse resource type from request path
fn parse_resource_type(path: &str) -> Option<ResourceType> {
    let path = path.to_lowercase();

    if path.contains("/devices") {
        Some(ResourceType::Devices)
    } else if path.contains("/models") {
        Some(ResourceType::Models)
    } else if path.contains("/deployments") {
        Some(ResourceType::Deployments)
    } else if path.contains("/users") {
        Some(ResourceType::Users)
    } else if path.contains("/audit-logs") {
        Some(ResourceType::AuditLogs)
    } else if path.contains("/alerts") {
        Some(ResourceType::Alerts)
    } else {
        None
    }
}

/// Parse action from HTTP method
fn parse_action(method: &axum::http::Method) -> Action {
    match method.as_str() {
        "GET" | "HEAD" => Action::Read,
        "POST" => Action::Create,
        "PUT" | "PATCH" => Action::Update,
        "DELETE" => Action::Delete,
        _ => Action::Read,
    }
}

/// RBAC middleware that checks permissions based on user role
pub async fn rbac_middleware(
    State(state): State<RbacState>,
    req: Request<axum::body::Body>,
    next: Next,
) -> Response {
    // Extract claims from request extensions (added by auth middleware)
    if let Some(claims) = req.extensions().get::<Claims>() {
        let resource = parse_resource_type(req.uri().path());
        let action = parse_action(req.method());

        if let Some(resource) = resource {
            let has_permission = state
                .check_permission(&claims.role, resource.clone(), action.clone())
                .await;

            if !has_permission {
                warn!(
                    "RBAC denied: user {} (role: {}) tried {:?} on {:?}",
                    claims.username, claims.role, action, resource
                );
                return (
                    axum::http::StatusCode::FORBIDDEN,
                    "Forbidden: Insufficient permissions",
                )
                    .into_response();
            }
        }
    }

    next.run(req).await
}

/// Helper macro to define permission requirements for routes
#[macro_export]
macro_rules! require_permission {
    ($role:expr, $resource:expr, $action:expr) => {
        axum::middleware::from_fn_with_state(
            $crate::rbac::RbacState::new(),
            $crate::rbac::rbac_middleware,
        )
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_resource_type() {
        assert_eq!(
            parse_resource_type("/api/devices"),
            Some(ResourceType::Devices)
        );
        assert_eq!(
            parse_resource_type("/api/models/upload"),
            Some(ResourceType::Models)
        );
        assert_eq!(
            parse_resource_type("/api/deployments/123"),
            Some(ResourceType::Deployments)
        );
        assert_eq!(
            parse_resource_type("/api/audit-logs"),
            Some(ResourceType::AuditLogs)
        );
        assert_eq!(
            parse_resource_type("/api/alerts"),
            Some(ResourceType::Alerts)
        );
    }

    #[test]
    fn test_parse_action() {
        assert_eq!(parse_action(&axum::http::Method::GET), Action::Read);
        assert_eq!(parse_action(&axum::http::Method::POST), Action::Create);
        assert_eq!(parse_action(&axum::http::Method::PUT), Action::Update);
        assert_eq!(parse_action(&axum::http::Method::DELETE), Action::Delete);
    }

    #[tokio::test]
    async fn test_admin_has_all_permissions() {
        let state = RbacState::new();

        assert!(
            state
                .check_permission("admin", ResourceType::Devices, Action::Delete)
                .await
        );
        assert!(
            state
                .check_permission("admin", ResourceType::Users, Action::Delete)
                .await
        );
    }

    #[tokio::test]
    async fn test_user_permissions() {
        let state = RbacState::new();

        // User can read devices
        assert!(
            state
                .check_permission("user", ResourceType::Devices, Action::Read)
                .await
        );

        // User cannot delete devices
        assert!(
            !state
                .check_permission("user", ResourceType::Devices, Action::Delete)
                .await
        );

        // User cannot access users resource
        assert!(
            !state
                .check_permission("user", ResourceType::Users, Action::Read)
                .await
        );
    }

    #[tokio::test]
    async fn test_device_operator_permissions() {
        let state = RbacState::new();

        // Device operator can read devices
        assert!(
            state
                .check_permission("device_operator", ResourceType::Devices, Action::Read)
                .await
        );

        // Device operator cannot create devices
        assert!(
            !state
                .check_permission("device_operator", ResourceType::Devices, Action::Create)
                .await
        );

        // Device operator cannot delete deployments
        assert!(
            !state
                .check_permission("device_operator", ResourceType::Deployments, Action::Delete)
                .await
        );
    }
}
