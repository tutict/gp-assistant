// Intentionally module memory only. No browser storage and no getter for OS secrets.
const sessionKeys = new Map<string, string>();
export function sessionCredential(id?: string): string | undefined { return id ? sessionKeys.get(id) : undefined; }
export function setSessionCredential(id: string, secret?: string): void {
  if (secret) sessionKeys.set(id, secret); else sessionKeys.delete(id);
}
export function clearSessionCredentials(): void { sessionKeys.clear(); }
