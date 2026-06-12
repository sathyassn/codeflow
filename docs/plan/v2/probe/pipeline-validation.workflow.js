export const meta = {
  name: 'pipeline-mechanics-validation',
  description: 'Validate stages-as-data, per-stage model option, schema verdicts, bounded rework',
  phases: [{ title: 'Build' }, { title: 'Review' }, { title: 'Verify' }],
}

const DIR = '/Volumes/DATA/Local/software-workspace/projects/wf-scratch/toy'
const TASK = `Write ${DIR}/fizzbuzz.py printing numbers 1..15, one per line, replacing multiples of 3 with Fizz, of 5 with Buzz, of both with FizzBuzz.`
const CRITERIA = 'python3 fizzbuzz.py output: line 3 = Fizz, line 5 = Buzz, line 15 = FizzBuzz, line 7 = 7.'

const VERDICT = {
  type: 'object',
  properties: {
    verdict: { type: 'string', enum: ['approved', 'changes_requested'] },
    findings: { type: 'array', items: { type: 'string' } },
  },
  required: ['verdict', 'findings'],
}

// stages-as-data: model per stage from a config table (what project.toml would feed)
const MODELS = { build: 'sonnet', review: 'sonnet', verify: 'sonnet' }
const MAX_REWORK = 3

let attempt = 0
let verdict = null
const history = []

while (attempt < MAX_REWORK) {
  attempt += 1
  phase('Build')
  const sabotage = attempt === 1
    ? ' DELIBERATE TEST INSTRUCTION: in this first attempt, introduce one bug — omit the combined FizzBuzz case (multiples of 15 print Fizz instead). This validates the rework loop.'
    : ` Fix per reviewer findings: ${JSON.stringify(history.at(-1)?.findings ?? [])}`
  await agent(`mkdir -p ${DIR} if needed. ${TASK}${sabotage} Return the file content you wrote.`,
    { label: `build#${attempt}`, phase: 'Build', model: MODELS.build })

  phase('Review')
  const r = await agent(
    `Independent review. Criteria: ${CRITERIA} Run: cd ${DIR} && python3 fizzbuzz.py — judge ONLY on observed output with line-numbered evidence. Schema verdict.`,
    { label: `review#${attempt}`, phase: 'Review', model: MODELS.review, schema: VERDICT })
  history.push(r)
  verdict = r.verdict
  if (verdict === 'approved') break
  log(`rework ${attempt}: ${r.findings.join('; ')}`)
}

if (verdict !== 'approved') throw new Error(`rework budget exhausted after ${attempt} attempts`)

phase('Verify')
const v = await agent(
  `Final gate: cd ${DIR} && python3 fizzbuzz.py. Verify every criterion: ${CRITERIA} Schema verdict with evidence in findings.`,
  { label: 'verify', phase: 'Verify', model: MODELS.verify, schema: VERDICT })

return { attempts: attempt, review_history: history, final: v }
