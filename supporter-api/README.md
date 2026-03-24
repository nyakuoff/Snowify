Snowify Supporter API (Stripe + Firebase)

This folder contains a minimal backend for optional Supporter subscriptions.

What it provides:
- Create Stripe checkout sessions
- Create Stripe customer-portal sessions
- Process Stripe webhooks
- Return supporter entitlements for a signed-in Firebase user

Endpoints:
- POST /api/billing/create-checkout-session
- POST /api/billing/create-portal-session
- GET /api/billing/entitlements
- POST /api/billing/webhook

Firestore writes:
- users/{uid}.supporter
- stripeCustomers/{customerId}

Supporter object schema:
- active: boolean
- plan: monthly | yearly | null
- stripeCustomerId: string | null
- stripeSubscriptionId: string | null
- currentPeriodEnd: number | null (unix ms)
- updatedAt: number (unix ms)

Run locally:
1) Copy .env.example to .env.local
2) Fill all required values
3) npm install
4) npm run dev

Deploy to Vercel:
1) vercel
2) Add env vars in Vercel project settings
3) Add Stripe webhook endpoint: https://YOUR_DOMAIN/api/billing/webhook
4) Subscribe webhook events:
   - checkout.session.completed
   - customer.subscription.created
   - customer.subscription.updated
   - customer.subscription.deleted
   - invoice.payment_failed

Client auth requirement:
- Send Firebase ID token in Authorization header:
  Authorization: Bearer <idToken>
