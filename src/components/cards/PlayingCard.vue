<script setup lang="ts">
import { computed } from 'vue';
import Icon, { type IconName } from '../Icon.vue';
import { useCardsText } from './cards.i18n';
import { faceFit, splitFaceValue, textFit } from '../../lib/cards/faceValue';
import type { GroupCoverage, GroupKind } from '../../lib/cards/deck';

const props = withDefaults(defineProps<{
  kind: 'day' | 'group';
  top: string;
  title: string;
  value?: string | null;
  unit?: string | null;
  corner?: string | null;
  dots?: Array<boolean | 'picked'> | null;
  state?: 'front' | 'confirm' | 'boxed';
  badge?: string | null;
  disabled?: boolean;
  empty?: boolean;
  holding?: boolean;
  focused?: boolean;
  tint?: string;
  icon?: IconName | null;
  value2?: string | null;
  unit2?: string | null;
  tint2?: string | null;
  deck?: GroupKind | null;
  coverage?: GroupCoverage;
  picked?: 'some' | 'all' | null;
}>(), {
  deck: null, coverage: null, picked: null, value2: null, unit2: null, tint2: null, value: null, unit: null, corner: null, dots: null, state: 'front', badge: null, disabled: false, empty: false, holding: false, focused: false, tint: 'var(--accent)', icon: null });
const t = useCardsText();
const parts = computed(() => splitFaceValue(props.value, props.unit));
const parts2 = computed(() => (props.tint2 ? splitFaceValue(props.value2, props.unit2) : []));
const fit = computed(() => ({ '--fit': String(Math.max(faceFit(parts.value), parts2.value.length ? faceFit(parts2.value) : 0)), '--tfit': String(textFit(props.title)), ...(props.tint2 ? { '--tint2': props.tint2 } : {}) }));
</script>

<template>
  <button type="button" :class="['pcard', kind, state, deck, picked ? `picked-${picked}` : null, coverage ? `cov-${coverage}` : null, { disabled, holding, focused, empty, duo: !!tint2 }]" :style="{ '--tint': tint, ...fit }" :aria-disabled="disabled || undefined">
    <span v-if="state === 'boxed'" class="pcard-face outline">
      <Icon name="box" :size="20" />
      <small>{{ t.inBox }}</small>
      <b>{{ title }}</b>
    </span>
    <span v-else-if="state === 'confirm'" class="pcard-face back">
      <span class="back-send">
        <span class="back-mark"><Icon name="check" :size="30" /></span>
        <strong>{{ t.picked }}</strong>
        <small class="back-title">{{ title }}</small>
      </span>
      <span class="back-cancel">{{ t.putBack }}</span>
    </span>
    <span v-else class="pcard-face">
      <span class="corner tl" aria-hidden="true"><small>{{ top }}</small><i></i></span>
      <span class="pcard-main">
        <Icon v-if="icon" class="pcard-icon" :name="icon" :size="22" />
        <strong class="pcard-title">{{ title }}</strong>
        <span v-if="kind === 'group' && empty" class="pcard-none-pill">{{ coverage === 'not_backfilled' ? t.notBackfilled : coverage === 'cloud_empty' ? t.cloudEmpty : t.noRecord }}</span>
        <template v-else-if="tint2 && kind === 'day'">
          <span :class="['pcard-value', { none: !parts.length }]"><template v-if="parts.length"><template v-for="(part, i) in parts" :key="i"><b v-if="part.n">{{ part.n }}</b><small v-else>{{ part.u }}</small></template></template><template v-else>—</template></span>
          <span :class="['pcard-value', 'second', { none: !parts2.length }]"><template v-if="parts2.length"><template v-for="(part, i) in parts2" :key="i"><b v-if="part.n">{{ part.n }}</b><small v-else>{{ part.u }}</small></template></template><template v-else>—</template></span>
        </template>
        <span v-else-if="parts.length" class="pcard-value"><template v-for="(part, i) in parts" :key="i"><b v-if="part.n">{{ part.n }}</b><small v-else>{{ part.u }}</small></template></span>
        <span v-else class="pcard-value none">—</span>
        <span v-if="dots && dots.length" :class="['pcard-dots', { 'has-picked': dots.includes('picked') }]" aria-hidden="true" :style="{ '--cols': Math.min(7, dots.length) }"><i v-for="(on, i) in dots" :key="i" :class="{ on: on === true, picked: on === 'picked' }"></i></span>
      </span>
      <span class="corner br" aria-hidden="true"><b v-if="corner">{{ corner }}</b><i></i></span>
      <em v-if="badge" class="pcard-badge">{{ badge }}</em>
    </span>
    <span class="pcard-glow" aria-hidden="true"></span>
    <span class="pcard-charge" aria-hidden="true"><i></i></span>
  </button>
</template>

<style scoped src="./PlayingCard.css"></style>
