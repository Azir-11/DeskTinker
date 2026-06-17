<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from "vue";
import { useMessage } from "naive-ui";
import dayjs from "dayjs";

const message = useMessage();

const FORMAT = "YYYY-MM-DD HH:mm:ss";

// 复制小标签的统一样式（一处定义，多处复用）。
const chip =
  "inline-flex items-center gap-4px px-9px py-4px rounded-sm border border-hairline bg-[var(--c-bg-0)] text-12px font-mono text-fg-2 cursor-pointer transition-colors hover:text-fg hover:border-hairline-strong";

// ── 当前时间戳（每秒刷新）──
const now = ref(Date.now());
let timer: number | undefined;
onMounted(() => {
  timer = window.setInterval(() => (now.value = Date.now()), 1000);
});
onUnmounted(() => {
  if (timer) clearInterval(timer);
});
const nowText = computed(() => dayjs(now.value).format(FORMAT));
const nowMs = computed(() => String(now.value));
const nowS = computed(() => String(Math.floor(now.value / 1000)));

// ── 时间戳 → 时间 ──
const tsInput = ref("");
const tsUnit = ref<"ms" | "s">("ms");
const tsResult = computed<{ ok: boolean; text: string } | null>(() => {
  const raw = tsInput.value.trim();
  if (!raw) return null;
  if (!/^\d+$/.test(raw)) return { ok: false, text: "请输入纯数字时间戳" };
  const ms = tsUnit.value === "s" ? Number(raw) * 1000 : Number(raw);
  const d = dayjs(ms);
  if (!d.isValid()) return { ok: false, text: "无效的时间戳" };
  return { ok: true, text: d.format(FORMAT) };
});

// ── 时间 → 时间戳 ──（NDatePicker 的值为毫秒时间戳）
const dateValue = ref<number | null>(Date.now());
const tsFromDate = computed<{ ms: string; s: string } | null>(() => {
  if (dateValue.value == null) return null;
  const d = dayjs(dateValue.value);
  if (!d.isValid()) return null;
  const ms = d.valueOf();
  return { ms: String(ms), s: String(Math.floor(ms / 1000)) };
});

async function copy(text: string) {
  try {
    await navigator.clipboard.writeText(text);
    message.success("已复制");
  } catch {
    message.info(text);
  }
}
function fillNowToInput() {
  tsInput.value = tsUnit.value === "s" ? nowS.value : nowMs.value;
}
</script>

<template>
  <div class="h-full overflow-auto pr-2px">
    <section class="card p-18px flex flex-col gap-16px w-full max-w-[520px]">
      <div class="flex items-center gap-10px">
        <span class="i-ph-clock-duotone text-20px text-accent-2 shrink-0" />
        <span class="text-14px font-600">时间戳转换</span>
      </div>

      <!-- 当前时间戳（实时） -->
      <div class="flex items-center gap-8px flex-wrap text-12px">
        <span class="text-fg-3">当前</span>
        <code class="font-mono text-fg">{{ nowText }}</code>
        <button :class="chip" title="复制毫秒" @click="copy(nowMs)">
          {{ nowMs }} <span class="i-ph-copy-bold" />
        </button>
        <button :class="chip" title="复制秒" @click="copy(nowS)">
          {{ nowS }}s <span class="i-ph-copy-bold" />
        </button>
      </div>

      <div class="h-1px bg-hairline" />

      <!-- 时间戳 → 时间 -->
      <div class="flex flex-col gap-9px">
        <span class="text-12px text-fg-3">时间戳 → 时间</span>
        <div class="flex items-center gap-10px flex-wrap">
          <NInput v-model:value="tsInput" placeholder="输入时间戳" clearable class="w-200px" />
          <NRadioGroup v-model:value="tsUnit" size="small">
            <NRadioButton value="ms">毫秒</NRadioButton>
            <NRadioButton value="s">秒</NRadioButton>
          </NRadioGroup>
          <NButton size="small" quaternary @click="fillNowToInput">此刻</NButton>
        </div>
        <div v-if="tsResult" class="flex items-center gap-8px">
          <span class="i-ph-arrow-right-bold text-13px text-fg-3 shrink-0" />
          <code v-if="tsResult.ok" class="font-mono text-15px text-accent-2">{{
            tsResult.text
          }}</code>
          <span v-else class="text-13px text-amber">{{ tsResult.text }}</span>
          <button v-if="tsResult.ok" :class="chip" title="复制" @click="copy(tsResult.text)">
            <span class="i-ph-copy-bold" />
          </button>
        </div>
      </div>

      <div class="h-1px bg-hairline" />

      <!-- 时间 → 时间戳 -->
      <div class="flex flex-col gap-9px">
        <span class="text-12px text-fg-3">时间 → 时间戳</span>
        <div class="flex items-center gap-10px flex-wrap">
          <NDatePicker v-model:value="dateValue" type="datetime" clearable class="w-240px" />
        </div>
        <div v-if="tsFromDate" class="flex items-center gap-8px flex-wrap">
          <button :class="chip" title="复制毫秒" @click="copy(tsFromDate.ms)">
            毫秒 {{ tsFromDate.ms }} <span class="i-ph-copy-bold" />
          </button>
          <button :class="chip" title="复制秒" @click="copy(tsFromDate.s)">
            秒 {{ tsFromDate.s }} <span class="i-ph-copy-bold" />
          </button>
        </div>
      </div>
    </section>
  </div>
</template>
