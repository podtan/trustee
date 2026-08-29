// Trustee default Cedar policy — P2 (nghr 645809c3).
//
// Default decision is DENY (pep CedarConfig DefaultDecision::Deny): any
// request not explicitly permitted here is rejected with 403. When Cedar
// is enabled, there is NO identity-only fallback.
//
// Role attribute form — the same one fame ships in production. The `has`
// guard is REQUIRED: accessing a missing attribute is an evaluation error
// (HTTP 500), whereas `has` simply evaluates false.
//     principal has role && principal.role == "admin"
//
// Role vocabulary (Kanidm claim maps — facts doc 42977cb7):
//   admin | user | service | agent
//
// NOTE for deployments with a filesystem policy override ([cedar] policy_path):
// this release REPLACES the single Action::"Access" with per-action permits.
// Old override files referencing Action::"Access" will deny everything —
// update them to the action names in trustee_schema.cedarschema.

// admin — everything, including actions added in future releases
// (unconstrained action matches the whole action set).
permit (
    principal,
    action,
    resource
)
when { principal has role && principal.role == "admin" };

// user (human team) — full session management, current and future.
permit (
    principal,
    action,
    resource
)
when { principal has role && principal.role == "user" };

// agent (agents-as-users) — the working set EXCEPT destructive
// DeleteSession (admin/user territory; revisit with task-F cutover
// evidence — loosening later is cheaper than un-deleting).
permit (
    principal,
    action in [
        Action::"ListModels",
        Action::"ListSessions",
        Action::"ViewSession",
        Action::"ViewHistory",
        Action::"CreateSession",
        Action::"CommandSession",
        Action::"CancelSession",
        Action::"HandoffSession",
        Action::"ResumeSession",
        Action::"UpdateSession",
        Action::"ViewMcpCredentials",
        Action::"UpdateMcpCredentials",
    ],
    resource
)
when { principal has role && principal.role == "agent" };

// service (tocpi-services identity) — read-only surface.
permit (
    principal,
    action in [
        Action::"ListModels",
        Action::"ListSessions",
        Action::"ViewSession",
        Action::"ViewHistory",
    ],
    resource
)
when { principal has role && principal.role == "service" };
