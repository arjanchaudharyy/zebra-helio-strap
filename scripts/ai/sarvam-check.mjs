// One Sarvam chat completion, to prove the key and model work.
// Run: node --env-file=.env scripts/ai/sarvam-check.mjs
import { SarvamAIClient } from 'sarvamai';

if (!process.env.SARVAM_API_KEY) {
  console.error('SARVAM_API_KEY is not set. Put it in .env.');
  process.exit(1);
}
const client = new SarvamAIClient({ apiSubscriptionKey: process.env.SARVAM_API_KEY });
const response = await client.chat.completionsV2({
  model: 'glm5.3',
  messages: [{ role: 'user', content: 'In one sentence: what does resting heart rate tell you about recovery?' }],
  max_tokens: 4000,
});
console.log(response.choices[0].message.content);
