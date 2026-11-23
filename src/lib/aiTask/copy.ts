import { backendText } from '../../i18n/backendText';
import { defineMessages, messagesOf } from '../../i18n';
import type { AiTaskIssue } from '../bridge/types';

const messages = defineMessages(
  {
    'ui.ai_task.cat.workout': 'Workouts in window',
    'ui.ai_task.cat.sleep': 'Sleep',
    'ui.ai_task.cat.recovery': 'Readiness',
    'ui.ai_task.cat.resting_hr': 'Resting HR',
    'ui.ai_task.cat.heart_rate': 'All-day heart rate',
    'ui.ai_task.cat.training': 'Training load',
    'ui.ai_task.cat.body': 'Body status',
    'ui.ai_task.cat.personal_note': 'Personal note',
    'ui.ai_task.cat.attachment': 'Attachments',

    'ui.ai_task.prompt.coverage_note':
      'The coverage below was measured on-device by ZeppBridge. Days without data are marked as missing — do not infer or invent them.',

    'ui.ai_task.blocked.attachment_missing': 'An original attachment can no longer be found. Reselect the file or remove the reference first.',
    'ui.ai_task.blocked.no_workouts': 'No workout is linked to this task yet. Go back and pick at least one.',
    'ui.ai_task.blocked.empty': 'The current selection covers no data. Adjust categories or workouts first.',

    'ui.ai_task.warn.attachment_changed': 'An attachment\'s size changed since it was added — confirm it is still the same file before handing off.',
    'ui.ai_task.warn.category_missing': 'This category has no data in the selected window; the export marks it as missing.',
    'ui.ai_task.warn.partial_coverage': 'Only part of the window has data.',
    'ui.ai_task.unknown': 'Unrecognized status note',

    'ui.ai_task.attach.no_redaction':
      'On export, originals are copied to the desktop handoff folder unredacted. Only share them with the chosen AI if you are comfortable with that.',

    'ui.ai_template.recovery_run.name': 'Recovery run',
    'ui.ai_template.recovery_run.prompt':
      'This was a recovery-phase workout. Using the two weeks of sleep, readiness and heart-rate context before it, assess whether the intensity matched my recovery level, and suggest training for the next 48 hours.',
    'ui.ai_template.long_run_compare.name': 'Long run comparison',
    'ui.ai_template.long_run_compare.prompt':
      'Compare these long runs: pace/heart-rate drift, perceived effort and recovery context. Which session was most efficient, and how should I set intensity for the next one?',
    'ui.ai_template.hr_drift.name': 'Heart-rate drift',
    'ui.ai_template.hr_drift.prompt':
      'Analyze the heart-rate drift in this workout: the rise at constant pace, judged against two weeks of sleep and training load — fatigue, weather, or fitness change?',

    'ui.ai_template.sleep_review.name': 'Recent sleep',
    'ui.ai_template.sleep_review.prompt':
      'Look back at my last two weeks of sleep: duration and how regular bedtime and wake time were, whether the share of deep and REM sleep changed, and how overnight heart rate and HRV moved. Point out the nights that stand out and suggest likely reasons using that day’s workouts and my notes.',
    'ui.ai_template.recovery_trend.name': 'Recovery trend',
    'ui.ai_template.recovery_trend.prompt':
      'Over the last four weeks, is my recovery getting better or worse? Look at resting heart rate, HRV, sleep and training load, whether they agree with each other, and separate real trends from day-to-day noise. Say plainly where data is missing.',
    'ui.ai_template.next_week.name': 'Plan next week',
    'ui.ai_template.next_week.prompt': 'Discuss next week using recent training, sleep, recovery and food. Allow changes across multiple turns; only return the final plan format when I say finalize.',
    'ui.ai_template.week_review.name': 'This week',
    'ui.ai_template.week_review.prompt':
      'Review this week: how sleep, recovery, heart rate and activity compare with usual, which changes are worth noticing and which are ordinary variation. Stick to the data, no diagnosis, and say plainly where data is missing.',

    fallbackIssue: 'A status note could not be recognized',
  },
);

const copy = () => messagesOf(messages) as Record<string, string>;

export const aiTaskTextFor = (code: string | null | undefined): string | undefined => {
  if (!code) return undefined;
  const value = copy()[code];
  return typeof value === 'string' && value ? value : undefined;
};

const ISSUE_CODE_ALIAS: Record<string, string> = {
  'err.ai_task.attachment_missing': 'ui.ai_task.blocked.attachment_missing',
};

export const aiTaskIssueText = (issue: AiTaskIssue, fallback = copy().fallbackIssue): string => {
  const { code, message: backendFallback } = issue;
  const lookupCode = code ? (ISSUE_CODE_ALIAS[code] ?? code) : code;
  return aiTaskTextFor(lookupCode) ?? backendText(backendFallback, fallback);
};

export const coverageNoteText = (): string => copy()['ui.ai_task.prompt.coverage_note'];

export const attachmentPlainReferenceNote = (): string => copy()['ui.ai_task.attach.no_redaction'];
