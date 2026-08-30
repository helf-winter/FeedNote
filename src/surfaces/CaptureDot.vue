<script setup lang="ts">
import { ref } from "vue";
import { LoaderCircle, Sparkles } from "lucide-vue-next";
import { prepareCapture } from "../api";

const busy = ref(false);

async function openCapture(): Promise<void> {
  if (busy.value) return;
  busy.value = true;
  try {
    await prepareCapture();
  } finally {
    busy.value = false;
  }
}
</script>

<template>
  <button
    class="capture-dot"
    :class="{ busy }"
    type="button"
    aria-label="投喂给 FeedNote"
    @click="openCapture"
  >
    <span class="capture-dot__core" aria-hidden="true">
      <LoaderCircle v-if="busy" class="capture-dot__spinner" :size="12" :stroke-width="2.4" />
      <Sparkles v-else :size="12" :stroke-width="2.4" />
    </span>
  </button>
</template>
