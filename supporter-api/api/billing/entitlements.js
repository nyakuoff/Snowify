const { requireUser } = require('../../lib/auth');
const { send, setCors } = require('../../lib/http');
const { getEntitlements } = require('../../lib/supporterStore');

module.exports = async (req, res) => {
  if (setCors(req, res)) return;
  if (req.method !== 'GET') return send(res, 405, { error: 'Method not allowed' });

  try {
    const user = await requireUser(req);
    const entitlements = await getEntitlements(user.uid);
    return send(res, 200, entitlements);
  } catch (err) {
    return send(res, 401, { error: err.message || 'Unauthorized' });
  }
};
