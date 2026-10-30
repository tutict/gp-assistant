// Intentionally module memory only. No browser storage and no getter for OS secrets.
const sessionKeys = new Map<string, string>();
const sessionVersions = new Map<string, number>();
export function sessionCredential(id?: string): string | undefined { return id ? sessionKeys.get(id) : undefined; }
export function sessionCredentialVersion(id?: string): number { return id ? sessionVersions.get(id) || 0 : 0; }
export function setSessionCredential(id: string, secret?: string): void {
  if (sessionKeys.get(id) !== (secret || undefined)) sessionVersions.set(id, (sessionVersions.get(id) || 0) + 1);
  if (secret) sessionKeys.set(id, secret); else sessionKeys.delete(id);
}
export function clearSessionCredentials(): void { for (const id of sessionKeys.keys()) setSessionCredential(id); }
