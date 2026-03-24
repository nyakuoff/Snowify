const Stripe = require('stripe');

let _stripe;

function getStripe() {
  if (_stripe) return _stripe;
  _stripe = new Stripe(process.env.STRIPE_SECRET_KEY, {
    apiVersion: '2024-06-20',
  });
  return _stripe;
}

module.exports = { getStripe };
