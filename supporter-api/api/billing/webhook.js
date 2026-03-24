const { getStripe } = require('../../lib/stripe');
const { send, setCors, readRawBody } = require('../../lib/http');
const {
  getUidByCustomerId,
  setSupporterFromSubscription,
  markSupporterInactive,
} = require('../../lib/supporterStore');

module.exports = async (req, res) => {
  if (setCors(req, res)) return;
  if (req.method !== 'POST') return send(res, 405, { error: 'Method not allowed' });

  const stripe = getStripe();
  const signature = req.headers['stripe-signature'];

  try {
    const raw = await readRawBody(req);
    const event = stripe.webhooks.constructEvent(raw, signature, process.env.STRIPE_WEBHOOK_SECRET);

    switch (event.type) {
      case 'checkout.session.completed': {
        const session = event.data.object;
        if (session.mode !== 'subscription') break;
        const customerId = session.customer;
        const uidFromMeta = session.metadata?.uid || null;
        const subscriptionId = session.subscription || null;

        if (!customerId || !subscriptionId) break;

        const subscription = await stripe.subscriptions.retrieve(subscriptionId);
        const uid = uidFromMeta || await getUidByCustomerId(customerId);
        if (!uid) break;

        await setSupporterFromSubscription(uid, customerId, subscription);
        break;
      }

      case 'customer.subscription.created':
      case 'customer.subscription.updated': {
        const subscription = event.data.object;
        const customerId = subscription.customer;
        const uidFromMeta = subscription.metadata?.uid || null;
        const uid = uidFromMeta || await getUidByCustomerId(customerId);
        if (!uid) break;

        await setSupporterFromSubscription(uid, customerId, subscription);
        break;
      }

      case 'customer.subscription.deleted':
      case 'invoice.payment_failed': {
        const obj = event.data.object;
        const customerId = obj.customer;
        const subscriptionId = obj.id || obj.subscription || null;
        const uid = await getUidByCustomerId(customerId);
        if (!uid) break;

        await markSupporterInactive(uid, customerId, subscriptionId);
        break;
      }

      default:
        break;
    }

    return send(res, 200, { received: true });
  } catch (err) {
    return send(res, 400, { error: `Webhook Error: ${err.message}` });
  }
};
