const { getStripe } = require('../../lib/stripe');
const { requireUser } = require('../../lib/auth');
const { send, setCors } = require('../../lib/http');
const { getOrCreateStripeCustomerId } = require('../../lib/supporterStore');

module.exports = async (req, res) => {
  if (setCors(req, res)) return;
  if (req.method !== 'POST') return send(res, 405, { error: 'Method not allowed' });

  try {
    const user = await requireUser(req);
    const stripe = getStripe();
    const customerId = await getOrCreateStripeCustomerId(user.uid, stripe, user.email);

    const session = await stripe.billingPortal.sessions.create({
      customer: customerId,
      return_url: process.env.APP_PORTAL_RETURN_URL || process.env.APP_SUCCESS_URL,
    });

    return send(res, 200, { url: session.url });
  } catch (err) {
    return send(res, 401, { error: err.message || 'Unauthorized' });
  }
};
