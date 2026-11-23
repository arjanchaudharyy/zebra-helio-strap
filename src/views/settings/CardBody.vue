<script setup lang="ts">
import AccountSection from './sections/AccountSection.vue';
import AdvancedSection from './sections/AdvancedSection.vue';
import ArchiveSection from './sections/ArchiveSection.vue';
import AutoSyncSection from './sections/AutoSyncSection.vue';
import CapabilitySection from './sections/CapabilitySection.vue';
import DevicesSection from './sections/DevicesSection.vue';
import DisplayPrefsSection from './sections/DisplayPrefsSection.vue';
import ExportDefaultsSection from './sections/ExportDefaultsSection.vue';
import GuideSection from './sections/GuideSection.vue';
import LabsSection from './sections/LabsSection.vue';
import McpSection from './sections/McpSection.vue';
import PrivacySection from './sections/PrivacySection.vue';

defineProps<{ id: string }>();
</script>

<template>
  <div class="card-body">
    <template v-if="id === 'account'">
      <AccountSection />
      <DevicesSection />
    </template>
    <AutoSyncSection v-else-if="id === 'sync'" />
    <ArchiveSection v-else-if="id === 'archive'" />
    <CapabilitySection v-else-if="id === 'data'" />
    <template v-else-if="id === 'ai'">
      <McpSection />
      <ExportDefaultsSection />
    </template>
    <DisplayPrefsSection v-else-if="id === 'display'" />
    <LabsSection v-else-if="id === 'labs'" />
    <GuideSection v-else-if="id === 'guide'" />
    <PrivacySection v-else-if="id === 'privacy'" />
    <AdvancedSection v-else-if="id === 'advanced'" />
  </div>
</template>
