// DanyWhere does not ship the upstream "handy" wordmark. This module keeps
// its upstream name and props so the call sites (Sidebar, onboarding, model
// migration) stay untouched and upstream merges stay conflict-free.
export { default } from "./DanyWhereWordmark";
