<script setup lang="ts">
import { watchEffect } from "vue";
import { darkTheme, dateZhCN, zhCN } from "naive-ui";
import { useAppStore } from "@/store/app";
import DefaultLayout from "@/layouts/default/index.vue";

const appStore = useAppStore();

// Reflect the active theme onto <html> so the CSS design tokens switch.
watchEffect(() => {
  document.documentElement.dataset.theme = appStore.isDark ? "dark" : "light";
});
</script>

<template>
  <NConfigProvider
    :theme="appStore.isDark ? darkTheme : null"
    :locale="zhCN"
    :date-locale="dateZhCN"
  >
    <NMessageProvider to="#overlay-host">
      <!-- 弹窗 teleport 进实体宿主：异形窗口靠 data-ct-solid 区分「可交互/可穿透」，
           而 Naive 弹窗默认挂到 <body>（无标记）会被点击穿透吞掉，无法点确认/取消。 -->
      <NDialogProvider to="#overlay-host">
        <DefaultLayout />
      </NDialogProvider>
    </NMessageProvider>
  </NConfigProvider>
  <div id="overlay-host" data-ct-solid />
</template>
