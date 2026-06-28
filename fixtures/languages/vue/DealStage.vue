<script setup lang="ts">
import { computed, ref } from "vue";

const props = defineProps<{ title: string }>();
const dealTitleId = computed(() => props.title.toLowerCase());
const retryCount = ref(0);
</script>

<template>
  <section>{{ dealTitleId }} {{ retryCount }}</section>
</template>
