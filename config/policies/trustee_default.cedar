// Trustee Default Cedar Policy
//
// DENY-BY-DEFAULT: No access is permitted unless explicitly allowed below.
// When Cedar is enabled (via [cedar] enabled = true in trustee.toml),
// every authenticated request is evaluated against these policies.
//
// To allow ALL authenticated users (Cedar acts as identity-only gate),
// uncomment this policy:
//
// permit(
//     principal,
//     action == Action::"Access",
//     resource
// );
//
// To restrict to specific users by their JWT subject (sub claim):
//
// permit(
//     principal == User::"<user-uuid-from-jwt-sub>",
//     action == Action::"Access",
//     resource
// );
//
// To restrict by role attribute:
//
// permit(
//     principal,
//     action == Action::"Access",
//     resource
// ) when {
//     principal has role && principal.role == "admin"
// };
//
// To restrict by group membership:
//
// permit(
//     principal,
//     action == Action::"Access",
//     resource
// ) when {
//     principal has groups && "trustee-users" in principal.groups
// };
