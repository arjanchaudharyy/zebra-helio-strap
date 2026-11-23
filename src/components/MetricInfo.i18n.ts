import { defineMessages, useMessages } from '../i18n';

export const metricInfoMessages = defineMessages(
  {
    about: (label: string) => `About ${label}`,
    sectionWhat: 'What this is',
    sectionChart: 'What this chart shows',
    sectionHow: 'How it is computed and where it comes from',
    sectionTrust: 'How it is measured',
    trustChipTitle: 'Black-box estimate',
    excludedChipTitle: 'You left this out of analysis',
    useInAnalysis: 'Use in analysis',
    useInAnalysisHint: 'When off, this still shows and no data is deleted; it is just left out of norm comparisons and is not sent to AI or MCP.',
    trustMeasured: 'Read directly by the sensor on the watch or device.',
    trustComputed: 'Converted from readings or your own logs with a published, fixed formula.',
    trustInferred: 'Worked out by the watch from its sensor readings in a familiar way (sleep from movement and heart rate, distance from steps). The details are not published, but it is fine for following trends.',
    trustEstimated: 'An estimate Zepp derives from several readings with its own algorithm. Zepp has not published the method or its error range, and lab measurements may differ.',
  },
);

export const useMetricInfoText = () => useMessages(metricInfoMessages);
