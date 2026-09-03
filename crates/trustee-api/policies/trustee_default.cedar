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

// agent (agents-as-users) — the working set. DeleteSession was granted
// 2026-09-01 (trustee 0.15.2): the THQ agent console's Terminate button
// dispatches AS the agent, so agent-without-delete made every console
// terminate 403. Containment: an agent token scopes every session route to
// HER OWN namespace — the grant only enables self-service cleanup of her
// own sessions, never another principal's.
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
        Action::"DeleteSession",
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
