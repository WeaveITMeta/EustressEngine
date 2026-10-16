// =============================================================================
// Who is an administrator
// =============================================================================
//
// One predicate, used wherever the Worker asks: an account is an administrator
// when it has the flag `admin:{id}` in KV or when its record says
// `role: 'admin'`. Two places asking two different questions would let a
// protection that names one of them be walked around by the other.
// =============================================================================

export async function isAdmin(env, userId) {
  if (typeof userId !== 'string' || !userId) return false;
  if (await env.USERS.get(`admin:${userId}`)) return true;
  const raw = await env.USERS.get(`user:${userId}`);
  if (!raw) return false;
  try {
    return JSON.parse(raw).role === 'admin';
  } catch {
    return false;
  }
}
