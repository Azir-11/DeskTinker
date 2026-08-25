<script setup lang="ts">
import { computed, reactive, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { openPath } from "@tauri-apps/plugin-opener";
import { useMessage } from "naive-ui";
import dayjs from "dayjs";

interface DomainRow {
  domain: string;
  wildcard: boolean;
}

interface ChallengeRecord {
  id: string;
  recordName: string;
  recordValue: string;
  domain: string;
  wildcard: boolean;
  display: string;
}

interface DnsCheckResult {
  recordId: string;
  domain: string;
  resolver: string;
  found: boolean;
  matched: boolean;
  values: string[];
}

interface DnsCheckSummary {
  recordId: string;
  domain: string;
  status: string;
  matchedCount: number;
  resolverCount: number;
  details: DnsCheckResult[];
}

interface AcmeStartResponse {
  orderId: string;
  caLabel: string;
  expires: string | null;
  san: string[];
  records: ChallengeRecord[];
}

interface IssuedCertInfo {
  orderId: string;
  ca: string;
  san: string[];
  exportDir: string;
  certPath: string;
  keyPath: string;
  combinedPath: string;
  issuedAt: number;
}

const message = useMessage();

const phase = ref<"form" | "records" | "done">("form");
const busyStart = ref(false);
const busyCheck = ref(false);
const busyFinalize = ref(false);
const pending = ref<AcmeStartResponse | null>(null);
const checks = ref<Record<string, DnsCheckSummary>>({});
const result = ref<IssuedCertInfo | null>(null);

const form = reactive({
  ca: "letsencrypt_staging",
  email: "",
  keyType: "ecdsa_p256",
  exportDir: "~/Desktop/hostguard-certs",
  rows: [{ domain: "", wildcard: false }] as DomainRow[],
});

const caOptions = [
  { label: "Let's Encrypt 测试（staging）", value: "letsencrypt_staging" },
  { label: "Let's Encrypt 正式", value: "letsencrypt_prod" },
];

const keyOptions = [
  { label: "ECC P-256（推荐）", value: "ecdsa_p256" },
  { label: "RSA 2048", value: "rsa2048" },
];

const resolverLabels: Record<string, string> = {
  "223.5.5.5": "阿里 DNS",
  "119.29.29.29": "DNSPod",
  "114.114.114.114": "114DNS",
  "8.8.8.8": "Google",
  "8.8.4.4": "Google",
  "1.1.1.1": "Cloudflare",
  "1.0.0.1": "Cloudflare",
};

const validForm = computed(() => {
  const hasDomain = form.rows.some((row) => row.domain.trim());
  return hasDomain && !!form.exportDir.trim();
});

const allMatched = computed(() => {
  if (!pending.value) return false;
  return pending.value.records.every((record) => checks.value[record.id]?.status === "matched");
});

function addRow() {
  form.rows.push({ domain: "", wildcard: false });
}

function removeRow(index: number) {
  if (form.rows.length > 1) form.rows.splice(index, 1);
}

async function startOrder() {
  if (!validForm.value) {
    message.warning("请填写至少一个域名和导出目录");
    return;
  }
  busyStart.value = true;
  try {
    const response = await invoke<AcmeStartResponse>("acme_start_order", {
      request: {
        ca: form.ca,
        email: form.email.trim(),
        domains: form.rows
          .filter((row) => row.domain.trim())
          .map((row) => ({ domain: row.domain.trim(), wildcard: row.wildcard })),
        keyType: form.keyType,
        exportDir: form.exportDir.trim(),
      },
    });
    pending.value = response;
    checks.value = {};
    phase.value = "records";
    message.success("challenge 信息已生成，请到 DNS 控制台添加 TXT 记录");
  } catch (error) {
    message.error(`生成 challenge 信息失败：${String(error)}`);
  } finally {
    busyStart.value = false;
  }
}

async function checkDns() {
  if (!pending.value) return;
  busyCheck.value = true;
  try {
    const list = await invoke<DnsCheckSummary[]>("acme_check_dns", {
      orderId: pending.value.orderId,
    });
    const map: Record<string, DnsCheckSummary> = {};
    list.forEach((summary) => {
      map[summary.recordId] = summary;
    });
    checks.value = map;
    if (list.length === 0) {
      message.info("没有待验证记录，可直接验证并签发");
      return;
    }
    const failed = list.filter((summary) => summary.status !== "matched").length;
    if (failed === 0) {
      message.success("全部域名解析已生效，可以验证并签发");
    } else {
      message.warning(
        `${list.length - failed}/${list.length} 条记录已生效，未生效的请检查 DNS 配置`,
      );
    }
  } catch (error) {
    message.error(`测试解析失败：${String(error)}`);
  } finally {
    busyCheck.value = false;
  }
}

async function finalizeOrder() {
  if (!pending.value || !allMatched.value) return;
  busyFinalize.value = true;
  try {
    result.value = await invoke<IssuedCertInfo>("acme_finalize_order", {
      orderId: pending.value.orderId,
    });
    phase.value = "done";
    message.success("证书签发成功");
  } catch (error) {
    message.error(`签发失败：${String(error)}`);
  } finally {
    busyFinalize.value = false;
  }
}

async function copyText(text: string) {
  try {
    await navigator.clipboard.writeText(text);
    message.success("已复制");
  } catch {
    message.error("复制失败，请手动选择复制");
  }
}

async function openExportDir() {
  if (!result.value) return;
  try {
    await openPath(result.value.exportDir);
  } catch {
    message.error("无法打开目录");
  }
}

function reset() {
  phase.value = "form";
  pending.value = null;
  checks.value = {};
  result.value = null;
}

function statusMeta(summary?: DnsCheckSummary): {
  label: string;
  type: "default" | "success" | "warning" | "error";
} {
  if (!summary) return { label: "待添加", type: "default" };
  switch (summary.status) {
    case "matched":
      return { label: "已生效", type: "success" };
    case "mismatch":
      return { label: "值不匹配", type: "error" };
    case "not_found":
      return { label: "未生效", type: "warning" };
    default:
      return { label: "待添加", type: "default" };
  }
}
</script>

<template>
  <div class="h-full flex flex-col gap-16px">
    <!-- 第一步：申请信息 -->
    <section v-if="phase === 'form'" class="flex flex-col gap-16px max-w-[760px]">
      <div class="card p-18px flex flex-col gap-14px">
        <div class="flex items-center gap-10px">
          <span class="i-ph-certificate-duotone text-20px text-accent-2 shrink-0" />
          <span class="text-14px font-600">证书申请</span>
          <span class="text-12px text-fg-3">手动 DNS-01 验证</span>
        </div>

        <div class="grid grid-cols-2 gap-12px">
          <div class="flex flex-col gap-6px">
            <label class="text-12px text-fg-3">证书颁发机构</label>
            <NSelect v-model:value="form.ca" :options="caOptions" to="#overlay-host" />
          </div>
          <div class="flex flex-col gap-6px">
            <label class="text-12px text-fg-3">账户邮箱（可选）</label>
            <NInput
              v-model:value="form.email"
              placeholder="用于 Let's Encrypt 账户通知"
              clearable
            />
          </div>
          <div class="flex flex-col gap-6px">
            <label class="text-12px text-fg-3">证书密钥类型</label>
            <NSelect v-model:value="form.keyType" :options="keyOptions" to="#overlay-host" />
          </div>
          <div class="flex flex-col gap-6px">
            <label class="text-12px text-fg-3">导出目录</label>
            <NInput
              v-model:value="form.exportDir"
              placeholder="~/Desktop/hostguard-certs"
              clearable
            />
          </div>
        </div>

        <div class="flex flex-col gap-8px">
          <div class="flex items-center justify-between">
            <label class="text-12px text-fg-3">域名（每行可同时签发通配符）</label>
            <NButton quaternary size="small" @click="addRow">
              <template #icon><span class="i-ph-plus-bold" /></template>
              添加域名
            </NButton>
          </div>
          <div v-for="(row, index) in form.rows" :key="index" class="flex items-center gap-10px">
            <NInput v-model:value="row.domain" placeholder="example.com" clearable class="flex-1" />
            <NCheckbox v-model:checked="row.wildcard">同时签发 *.</NCheckbox>
            <NButton
              v-if="form.rows.length > 1"
              quaternary
              circle
              size="small"
              @click="removeRow(index)"
            >
              <template #icon><span class="i-ph-x-bold" /></template>
            </NButton>
          </div>
        </div>

        <div class="flex items-center gap-10px flex-wrap">
          <NButton type="primary" :loading="busyStart" :disabled="!validForm" @click="startOrder">
            <template #icon><span class="i-ph-arrow-clockwise-bold" /></template>
            生成challenge信息
          </NButton>
          <span class="text-12px text-fg-3 leading-[1.5]">
            生成后将得到每个域名的 TXT 记录，请到 DNS 控制台手动添加
          </span>
        </div>
      </div>
    </section>

    <!-- 第二步：挑战记录与解析测试 -->
    <section v-else-if="phase === 'records'" class="flex flex-col gap-14px min-h-0 flex-1">
      <div class="flex items-center gap-10px flex-wrap">
        <NButton quaternary @click="reset">
          <template #icon><span class="i-ph-arrow-left-duotone" /></template>
          返回
        </NButton>
        <div class="flex-1" />
        <span v-if="pending?.expires" class="text-12px text-fg-3">
          建议在 {{ dayjs(pending.expires).format("MM-DD HH:mm") }} 前完成
        </span>
        <NButton quaternary :loading="busyStart" @click="startOrder">
          <template #icon><span class="i-ph-arrow-clockwise-bold" /></template>
          重新生成
        </NButton>
        <NButton type="primary" :loading="busyCheck" @click="checkDns">
          <template #icon><span class="i-ph-magnifying-glass-bold" /></template>
          测试解析
        </NButton>
        <NButton
          type="primary"
          secondary
          :disabled="!allMatched"
          :loading="busyFinalize"
          @click="finalizeOrder"
        >
          <template #icon><span class="i-ph-shield-check-duotone" /></template>
          验证并签发
        </NButton>
      </div>

      <div v-if="pending" class="flex flex-col gap-10px">
        <div class="flex items-center gap-8px flex-wrap">
          <span class="text-12px text-fg-3">本证书 SAN：</span>
          <NTag v-for="domain in pending.san" :key="domain" size="small" :bordered="false">
            {{ domain }}
          </NTag>
        </div>

        <div
          v-if="pending.records.length === 0"
          class="rounded-lg border border-hairline bg-elevated p-14px flex items-center gap-10px"
        >
          <span class="i-ph-shield-check-duotone text-18px text-accent-2 shrink-0" />
          <span class="text-13px text-fg-2">
            所有域名此前已通过验证，无需添加 TXT 记录，可直接验证并签发
          </span>
        </div>

        <div
          v-for="record in pending.records"
          :key="record.id"
          class="rounded-lg border border-hairline bg-elevated p-14px flex flex-col gap-10px"
        >
          <div class="flex items-center gap-10px flex-wrap">
            <span class="i-ph-globe-hemisphere-west-duotone text-16px text-accent-2 shrink-0" />
            <strong class="text-13px font-600">{{ record.display }}</strong>
            <NTag size="small" :bordered="false" :type="statusMeta(checks[record.id]).type">
              {{ statusMeta(checks[record.id]).label }}
            </NTag>
            <span v-if="checks[record.id]" class="text-11px text-fg-3">
              {{ checks[record.id].matchedCount }}/{{ checks[record.id].resolverCount }} 解析器通过
            </span>
          </div>

          <div class="flex flex-col gap-6px min-w-0">
            <div class="grid grid-cols-[72px_minmax(0,1fr)_auto] items-center gap-8px">
              <span class="text-11px text-fg-3">主机记录</span>
              <code class="min-w-0 text-12px text-fg-2 font-mono break-all select-text">
                {{ record.recordName }}
              </code>
              <NButton
                quaternary
                circle
                size="small"
                title="复制"
                @click="copyText(record.recordName)"
              >
                <template #icon><span class="i-ph-copy-bold" /></template>
              </NButton>
            </div>
            <div class="grid grid-cols-[72px_minmax(0,1fr)_auto] items-center gap-8px">
              <span class="text-11px text-fg-3">记录值</span>
              <code class="min-w-0 text-12px text-fg-2 font-mono break-all select-text">
                {{ record.recordValue }}
              </code>
              <NButton
                quaternary
                circle
                size="small"
                title="复制"
                @click="copyText(record.recordValue)"
              >
                <template #icon><span class="i-ph-copy-bold" /></template>
              </NButton>
            </div>
          </div>

          <div v-if="checks[record.id]" class="flex flex-wrap gap-6px">
            <span
              v-for="detail in checks[record.id].details"
              :key="detail.resolver"
              class="inline-flex items-center gap-5px text-11px px-8px py-3px rounded-md border border-hairline"
              :class="detail.matched ? 'text-accent-2' : detail.found ? 'text-amber' : 'text-fg-3'"
            >
              <span
                :class="
                  detail.matched
                    ? 'i-ph-check-bold'
                    : detail.found
                      ? 'i-ph-warning-circle-duotone'
                      : 'i-ph-x-bold'
                "
              />
              {{ resolverLabels[detail.resolver] ?? detail.resolver }}
            </span>
          </div>
          <p v-else class="m-0 text-11px text-fg-3 leading-[1.5]">
            在 DNS 控制台添加 TXT 记录后，点击「测试解析」确认生效
          </p>
        </div>
      </div>
    </section>

    <!-- 第三步：签发结果 -->
    <section v-else-if="phase === 'done' && result" class="flex flex-col gap-16px max-w-[680px]">
      <div class="flex flex-col items-start gap-12px">
        <div class="flex items-center gap-10px">
          <span class="i-ph-shield-check-duotone text-26px text-accent-2 shrink-0" />
          <div class="flex flex-col gap-2px">
            <div class="text-15px font-600">证书签发成功</div>
            <div class="text-12px text-fg-3">SAN：{{ result.san.join("、") }}</div>
          </div>
        </div>

        <div
          class="w-full rounded-lg border border-hairline bg-elevated p-14px flex flex-col gap-8px"
        >
          <div class="grid grid-cols-[88px_minmax(0,1fr)] gap-8px items-center">
            <span class="text-12px text-fg-3">证书链</span>
            <code class="text-12px text-fg-2 font-mono break-all select-text">
              {{ result.certPath }}
            </code>
          </div>
          <div class="grid grid-cols-[88px_minmax(0,1fr)] gap-8px items-center">
            <span class="text-12px text-fg-3">私钥</span>
            <code class="text-12px text-fg-2 font-mono break-all select-text">
              {{ result.keyPath }}
            </code>
          </div>
          <div class="grid grid-cols-[88px_minmax(0,1fr)] gap-8px items-center">
            <span class="text-12px text-fg-3">合并文件</span>
            <code class="text-12px text-fg-2 font-mono break-all select-text">
              {{ result.combinedPath }}
            </code>
          </div>
        </div>

        <div class="flex items-center gap-10px">
          <NButton type="primary" @click="openExportDir">
            <template #icon><span class="i-ph-folder-open-duotone" /></template>
            打开目录
          </NButton>
          <NButton @click="reset">
            <template #icon><span class="i-ph-plus-bold" /></template>
            继续申请
          </NButton>
        </div>
        <p class="m-0 text-12px text-fg-3 leading-[1.6]">
          验证已通过，可到 DNS 控制台删除对应的 TXT 记录
        </p>
      </div>
    </section>
  </div>
</template>
