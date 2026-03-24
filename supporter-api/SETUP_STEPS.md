Snowify Stripe Setup Steps (Owner Actions)

1) Stripe Dashboard setup
- Create product: Snowify Supporter
- Create recurring prices:
  - Monthly
  - Yearly
- Copy both price IDs

2) Stripe Customer Portal
- Enable cancellation, plan switching, payment method updates

3) Backend deploy (Vercel)
- In this folder run:
  npm install
  npx vercel
- Finish first deploy and note your deployed URL

4) Vercel environment variables (Project Settings)
- STRIPE_SECRET_KEY
- STRIPE_WEBHOOK_SECRET (set after step 5)
- STRIPE_PRICE_MONTHLY
- STRIPE_PRICE_YEARLY
- APP_SUCCESS_URL
- APP_CANCEL_URL
- APP_PORTAL_RETURN_URL
- FIREBASE_PROJECT_ID
- FIREBASE_CLIENT_EMAIL
- FIREBASE_PRIVATE_KEY
- ALLOWED_ORIGIN (optional, can be *)

5) Stripe webhook
- Add endpoint: https://YOUR_DOMAIN/api/billing/webhook
- Events:
  - checkout.session.completed
  - customer.subscription.created
  - customer.subscription.updated
  - customer.subscription.deleted
  - invoice.payment_failed
- Copy webhook signing secret and place in STRIPE_WEBHOOK_SECRET
- Redeploy after setting this value

6) Firestore rules
- Ensure client cannot write users/{uid}.supporter
- Client can read their own supporter state
- Backend service account writes supporter state

7) Manual endpoint test (quick)
- GET /api/billing/entitlements with Bearer Firebase ID token
- POST /api/billing/create-checkout-session with body {"plan":"monthly"}
- POST /api/billing/create-portal-session

8) Tell Copilot when done
- Share deployed backend URL
- Confirm monthly/yearly price IDs are set
- Confirm webhook is active
- Confirm Firestore rules are updated

Then app integration can be implemented in Snowify renderer/main code.
