// Tiny helpers shared by GitView and the file status pills.

export const statusColors: Record<string, string> = {
  added: "var(--green)",
  modified: "var(--yellow)",
  deleted: "var(--red)",
  renamed: "var(--cyan)",
  typechange: "var(--purple)",
  untracked: "var(--blue)",
  ignored: "var(--fg-3)",
};

export function statusLabel(kind: string): string {
  switch (kind) {
    case "added":
      return "added";
    case "modified":
      return "modified";
    case "deleted":
      return "deleted";
    case "renamed":
      return "renamed";
    case "typechange":
      return "type changed";
    case "untracked":
      return "untracked";
    case "ignored":
      return "ignored";
    default:
      return "—";
  }
}
