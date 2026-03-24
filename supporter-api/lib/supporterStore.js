const { getFirestore } = require('./firebaseAdmin');

function userRef(uid) {
  return getFirestore().collection('users').doc(uid);
}

function stripeCustomerRef(customerId) {
  return getFirestore().collection('stripeCustomers').doc(customerId);
}

async function getOrCreateStripeCustomerId(uid, stripe, email) {
  const snap = await userRef(uid).get();
  const data = snap.exists ? snap.data() : {};
  const existing = data?.supporter?.stripeCustomerId;
  if (existing) return existing;

  const customer = await stripe.customers.create({
    email: email || undefined,
    metadata: { uid },
  });

  await userRef(uid).set({
    supporter: {
      active: false,
      plan: null,
      stripeCustomerId: customer.id,
      stripeSubscriptionId: null,
      currentPeriodEnd: null,
      updatedAt: Date.now(),
    },
  }, { merge: true });

  await stripeCustomerRef(customer.id).set({ uid, updatedAt: Date.now() }, { merge: true });

  return customer.id;
}

async function getUidByCustomerId(customerId) {
  const byMap = await stripeCustomerRef(customerId).get();
  if (byMap.exists) return byMap.data().uid;

  const q = await getFirestore().collection('users')
    .where('supporter.stripeCustomerId', '==', customerId)
    .limit(1)
    .get();

  if (q.empty) return null;
  return q.docs[0].id;
}

function planFromPriceId(priceId) {
  if (!priceId) return null;
  if (priceId === process.env.STRIPE_PRICE_MONTHLY) return 'monthly';
  if (priceId === process.env.STRIPE_PRICE_YEARLY) return 'yearly';
  return null;
}

async function setSupporterFromSubscription(uid, customerId, subscription) {
  const item = subscription.items?.data?.[0];
  const priceId = item?.price?.id || null;
  const plan = planFromPriceId(priceId);
  const active = ['active', 'trialing', 'past_due'].includes(subscription.status);
  const endSec = subscription.current_period_end || null;

  await userRef(uid).set({
    supporter: {
      active,
      plan,
      stripeCustomerId: customerId,
      stripeSubscriptionId: subscription.id || null,
      currentPeriodEnd: endSec ? endSec * 1000 : null,
      updatedAt: Date.now(),
    },
  }, { merge: true });

  if (customerId) {
    await stripeCustomerRef(customerId).set({ uid, updatedAt: Date.now() }, { merge: true });
  }
}

async function markSupporterInactive(uid, customerId, subscriptionId) {
  await userRef(uid).set({
    supporter: {
      active: false,
      stripeCustomerId: customerId || null,
      stripeSubscriptionId: subscriptionId || null,
      updatedAt: Date.now(),
    },
  }, { merge: true });

  if (customerId) {
    await stripeCustomerRef(customerId).set({ uid, updatedAt: Date.now() }, { merge: true });
  }
}

async function getEntitlements(uid) {
  const snap = await userRef(uid).get();
  const data = snap.exists ? snap.data() : {};
  const s = data?.supporter || {};

  return {
    supporter: {
      active: !!s.active,
      plan: s.plan || null,
      stripeCustomerId: s.stripeCustomerId || null,
      stripeSubscriptionId: s.stripeSubscriptionId || null,
      currentPeriodEnd: s.currentPeriodEnd || null,
      updatedAt: s.updatedAt || null,
    },
    perks: {
      gifMedia: !!s.active,
      supporterBadge: !!s.active,
      largerBio: !!s.active,
      usernameStyling: !!s.active,
      crossfade: !!s.active,
      listenAlongEligible: !!s.active,
    },
  };
}

module.exports = {
  getOrCreateStripeCustomerId,
  getUidByCustomerId,
  setSupporterFromSubscription,
  markSupporterInactive,
  getEntitlements,
};
