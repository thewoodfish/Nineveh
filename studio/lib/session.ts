// The Nineveh session Studio signs in with (ADR 0018): a bearer token, kept in this
// browser and sent on every request. A token that stops working brings back the
// sign-in screen.

const KEY = "nineveh.session";
const EXPIRED = "nineveh:signed-out";

export function session(): string | null {
  if (typeof window === "undefined") return null;
  try {
    return window.localStorage.getItem(KEY);
  } catch {
    return null;
  }
}

export function setSession(token: string) {
  try {
    window.localStorage.setItem(KEY, token);
  } catch {
    // Storage blocked: the session lasts this page only.
  }
}

export function clearSession() {
  try {
    window.localStorage.removeItem(KEY);
  } catch {
    // Nothing stored.
  }
}

/** The session was refused: sign in again. */
export function signedOut() {
  clearSession();
  window.dispatchEvent(new Event(EXPIRED));
}

export function onSignedOut(listener: () => void): () => void {
  window.addEventListener(EXPIRED, listener);
  return () => window.removeEventListener(EXPIRED, listener);
}
