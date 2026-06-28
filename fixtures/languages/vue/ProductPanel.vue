<script setup lang="ts">
import { computed, ref } from "vue";

const props = defineProps<{ title: string }>();
const productTitleId = computed(() => props.title.toLowerCase());
const retryCount = ref(0);
</script>

<template>
  <section>{{ productTitleId }} {{ retryCount }}</section>
</template>
