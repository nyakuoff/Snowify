const { getStripe } = require('../../lib/stripe');
const { requireUser } = require('../../lib/auth');
const { send, setCors, readJsonBody } = require('../../lib/http');
const { getOrCreateStripeCustomerId } = require('../../lib/supporterStore');

function resolvePrice(plan) {
  if (plan === 'monthly') return process.env.STRIPE_PRICE_MONTHLY;
  if (plan === 'yearly') return process.env.STRIPE_PRICE_YEARLY;
  return null;
}

module.exports = async (req, res) => {
  if (setCors(req, res)) return;
  if (req.method !== 'POST') return send(res, 405, { error: 'Method not allowed' });

  try {
    const user = await requireUser(req);
    const body = await readJsonBody(req);
    const plan = body.plan;
    const price = resolvePrice(plan);

    if (!price) return send(res, 400, { error: 'Invalid plan' });

    const stripe = getStripe();
    const customerId = await getOrCreateStripeCustomerId(user.uid, stripe, user.email);

    const session = await stripe.checkout.sessions.create({
      mode: 'subscription',
      customer: customerId,
      line_items: [{ price, quantity: 1 }],
      success_url: process.env.APP_SUCCESS_URL,
      cancel_url: process.env.APP_CANCEL_URL,
      allow_promotion_codes: true,
      metadata: { uid: user.uid, plan },
      subscription_data: { metadata: { uid: user.uid } },
    });

    return send(res, 200, { url: session.url, sessionId: session.id });
  } catch (err) {
    return send(res, 401, { error: err.message || 'Unauthorized' });
  }
};
