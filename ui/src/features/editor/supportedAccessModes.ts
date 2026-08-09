import type { AccessMode } from "@/lib/types/workflow";

export function supportedAccessModes(accessProfiles: string[] | undefined): AccessMode[] {
  if (!accessProfiles) {
    return ["read_only", "edit", "execute", "unrestricted"];
  }
  const profiles = new Set(accessProfiles);
  const modes: AccessMode[] = [];
  if (profiles.has("read-only")) modes.push("read_only");
  if (profiles.has("workspace-write")) {
    modes.push("edit", "execute");
  } else if (profiles.has("default")) {
    modes.push("execute");
  }
  if (profiles.has("full-access")) modes.push("unrestricted");
  return modes;
}
