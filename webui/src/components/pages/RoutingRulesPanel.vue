<script setup lang="ts">
import { computed, onBeforeUnmount, ref } from "vue";
import { ArrowDown, ArrowUp, Pencil, Plus, RefreshCw, Trash2 } from "lucide-vue-next";
import { t } from "@/i18n";
import Button from "@/components/ui/Button.vue";
import Card from "@/components/ui/Card.vue";
import CardHeading from "@/components/ui/CardHeading.vue";
import ConfirmPanel from "@/components/ui/ConfirmPanel.vue";
import Textarea from "@/components/ui/Textarea.vue";
import { useActionLock } from "@/composables/useActionLock";
import { useMagicNet } from "@/composables/useMagicNet";
import { useVisibilityTask } from "@/composables/useVisibilityTask";
import {
  buildRoutingPayload, copyRoutingPolicy, parseRoutingPolicySnapshot, renameRoutingGroup,
  routingGroupReferenced, routingTargets, supportsRoutingPolicy, validateRoutingConditions,
  type RoutingPolicy, type RoutingPolicyGroup, type RoutingPolicySnapshot,
} from "./routingPolicyPlan";

const { runCli, runPrivateCli, stagePrivatePayload, removePrivatePayload, shellQuote } = useMagicNet();
const { isRunning, withAction } = useActionLock();
const emit = defineEmits<{ applied: [] }>();
const snapshot = ref<RoutingPolicySnapshot | null>(null);
const policy = ref<RoutingPolicy>({ schema: 1, rules: [], groups: [], final: "proxy-auto" });
const query = ref("");
const ruleDraft = ref<{ index: number | null; conditions: string; target: string } | null>(null);
const groupDraft = ref<{ index: number | null; tag: string; type: "selector" | "urltest"; members: string[]; default: string; url: string } | null>(null);
const pendingDelete = ref<{ kind: "rule" | "group"; index: number } | null>(null);
const pendingApply = ref(false);
const error = ref("");
const status = ref("");
let disposed = false;
let sequence = 0;
let capabilityChecked = false;
const busy = computed(() => isRunning("routing-policy-load") || isRunning("routing-policy-save"));
const mutationLocked = computed(() => busy.value || Boolean(pendingApply.value || pendingDelete.value || ruleDraft.value || groupDraft.value));
const changed = computed(() => snapshot.value && JSON.stringify(policy.value) !== JSON.stringify(copyRoutingPolicy(snapshot.value)));
const targets = computed(() => snapshot.value ? routingTargets(snapshot.value, policy.value) : []);
const rows = computed(() => policy.value.rules.map((rule, index) => ({ rule, index })).filter(({ rule }) =>
  `${JSON.stringify(rule.conditions)} ${rule.target}`.toLowerCase().includes(query.value.trim().toLowerCase())));
const memberTargets = computed(() => targets.value.filter((target) => target.tag !== groupDraft.value?.tag
  && (groupDraft.value?.type !== "urltest" || !["selector", "urltest", "direct", "block"].includes(target.type))));

async function load(force = false): Promise<boolean> {
  if (!force && (changed.value || ruleDraft.value || groupDraft.value || pendingApply.value)) return false;
  const current = ++sequence;
  if (!capabilityChecked) {
    const text = await runCli("--json capabilities", t("读取路由管理能力"), true);
    if (disposed || current !== sequence) return false;
    if (!supportsRoutingPolicy(text)) { error.value = t("当前模块不支持路由策略管理，请升级模块。"); return false; }
    capabilityChecked = true;
  }
  const result = await runPrivateCli("--json routing inspect", t("读取路由配置"), "--json routing inspect");
  if (disposed || current !== sequence || (!force && (changed.value || ruleDraft.value || groupDraft.value))) return false;
  const parsed = result.ok ? parseRoutingPolicySnapshot(result.stdout) : null;
  if (!parsed) { error.value = t("无法读取路由策略，请检查配置或健康状态。"); return false; }
  snapshot.value = parsed;
  policy.value = copyRoutingPolicy(parsed);
  error.value = "";
  return true;
}

async function refresh(): Promise<void> { await withAction("routing-policy-load", async () => { await load(); }); }

function editRule(index: number | null): void {
  if (busy.value || pendingApply.value || pendingDelete.value || !snapshot.value) return;
  const rule = index === null ? null : policy.value.rules[index];
  ruleDraft.value = { index, conditions: JSON.stringify(rule?.conditions ?? { domain_suffix: ["example.com"] }, null, 2), target: rule?.target ?? "proxy-auto" };
  groupDraft.value = null;
  pendingApply.value = false;
}

function keepRule(): void {
  if (!ruleDraft.value || !snapshot.value) return;
  try {
    const conditions: unknown = JSON.parse(ruleDraft.value.conditions);
    validateRoutingConditions(conditions, snapshot.value.ruleSets);
    if (!targets.value.some((target) => target.tag === ruleDraft.value?.target)) throw new Error(t("请选择可用的目标出站。"));
    const rule = { conditions, target: ruleDraft.value.target };
    if (ruleDraft.value.index === null) policy.value.rules.push(rule);
    else policy.value.rules.splice(ruleDraft.value.index, 1, rule);
    ruleDraft.value = null;
    error.value = "";
  } catch (failure) { error.value = failure instanceof SyntaxError ? t("匹配条件不是有效 JSON。") : String(failure instanceof Error ? failure.message : failure); }
}

function move(index: number, direction: number): void {
  const destination = index + direction;
  if (busy.value || pendingApply.value || pendingDelete.value || ruleDraft.value || groupDraft.value || destination < 0 || destination >= policy.value.rules.length) return;
  const [rule] = policy.value.rules.splice(index, 1);
  policy.value.rules.splice(destination, 0, rule!);
  pendingApply.value = false;
}

function editGroup(index: number | null): void {
  if (busy.value || pendingApply.value || pendingDelete.value || !snapshot.value) return;
  const group = index === null ? null : policy.value.groups[index];
  groupDraft.value = { index, tag: group?.tag ?? "", type: group?.type ?? "selector", members: [...(group?.members ?? ["proxy-auto"])], default: group?.default ?? "proxy-auto", url: group?.url ?? "https://www.gstatic.com/generate_204" };
  ruleDraft.value = null;
  pendingApply.value = false;
}

function keepGroup(): void {
  if (!groupDraft.value || !snapshot.value) return;
  const draft = groupDraft.value;
  const next = copyRoutingPolicy(policy.value);
  const group: RoutingPolicyGroup = { tag: draft.tag.trim(), type: draft.type, members: [...draft.members] };
  if (group.type === "selector") group.default = draft.default;
  else group.url = draft.url.trim();
  if (draft.index === null) next.groups.push(group);
  else {
    const old = next.groups[draft.index]!.tag;
    next.groups.splice(draft.index, 1, group);
    if (old !== group.tag) renameRoutingGroup(next, old, group.tag);
  }
  try {
    buildRoutingPayload(snapshot.value, next);
    policy.value = next;
    groupDraft.value = null;
    error.value = "";
  } catch (failure) { error.value = failure instanceof Error ? failure.message : String(failure); }
}

function requestDelete(kind: "rule" | "group", index: number): void {
  if (mutationLocked.value || !snapshot.value) return;
  pendingDelete.value = { kind, index };
}

function deleteEntry(): void {
  if (!pendingDelete.value || busy.value) return;
  const { kind, index } = pendingDelete.value;
  if (kind === "group") {
    const group = policy.value.groups[index];
    if (group && routingGroupReferenced(policy.value, group.tag)) { error.value = t("策略组仍被引用，请先修改规则、成员或兜底出站。"); pendingDelete.value = null; return; }
    policy.value.groups.splice(index, 1);
  } else policy.value.rules.splice(index, 1);
  pendingDelete.value = null;
  pendingApply.value = false;
}

function discard(): void {
  if (snapshot.value) policy.value = copyRoutingPolicy(snapshot.value);
  ruleDraft.value = null;
  groupDraft.value = null;
  pendingApply.value = false;
  pendingDelete.value = null;
  error.value = "";
}

function requestApply(): void {
  if (!snapshot.value || mutationLocked.value) return;
  try { buildRoutingPayload(snapshot.value, policy.value); pendingApply.value = true; error.value = ""; }
  catch (failure) { error.value = failure instanceof Error ? failure.message : String(failure); }
}

async function apply(): Promise<void> {
  if (!snapshot.value || !pendingApply.value || busy.value) return;
  let payload: string;
  try { payload = JSON.stringify(buildRoutingPayload(snapshot.value, policy.value)); }
  catch (failure) { error.value = failure instanceof Error ? failure.message : String(failure); pendingApply.value = false; return; }
  await withAction("routing-policy-save", async () => {
    const basename = `routing-policy-${Date.now()}-${Math.random().toString(36).slice(2, 8)}.json`;
    const staged = await stagePrivatePayload("tmp", basename, payload, t("路由策略私有载荷"));
    if (!staged) { if (!disposed) error.value = t("安全临时数据写入失败，路由策略未保存。"); return; }
    let refreshed = false;
    try {
      if (disposed) return;
      const result = await runPrivateCli(`routing save-file ${shellQuote(staged.path)}`, t("校验并应用路由策略"), "routing save-file [private-payload]");
      if (disposed) return;
      pendingApply.value = false;
      if (!result.ok) { error.value = t("路由策略校验或应用失败，请重新读取配置并检查健康状态。"); return; }
      refreshed = await load(true);
    } finally {
      const cleaned = await removePrivatePayload("tmp", staged.basename, t("路由策略私有载荷"));
      if (!cleaned && !disposed) error.value = t("路由策略临时载荷清理失败，请检查模块临时目录。");
    }
    if (!disposed && refreshed) {
      status.value = t("路由策略已保存并应用，订阅刷新会保留用户设置。");
      emit("applied");
    }
  });
}

const { target: visibilityTarget } = useVisibilityTask(refresh);
onBeforeUnmount(() => { disposed = true; ++sequence; });
</script>

<template>
  <div ref="visibilityTarget" class="mn-deferred-region routing-policy-panel">
    <Card class="grid gap-3">
      <CardHeading :title="t('路由规则')">
        <Button size="sm" variant="outline" :disabled="Boolean(changed || ruleDraft || groupDraft || pendingApply)" :loading="busy" @click="refresh"><RefreshCw :size="15" />{{ t("刷新") }}</Button>
      </CardHeading>
      <p class="text-sm text-[var(--mn-ink-muted)]">{{ t("规则直接选择节点或策略组，按顺序首次匹配。系统防护由模块维护；业务规则可增删改查，策略组按需创建。") }}</p>
      <p v-if="snapshot?.source === 'defaults'" class="text-xs text-[var(--mn-ink-muted)]">{{ t("当前展示精简默认策略，保存后成为持久化用户配置。") }}</p>
      <p v-if="snapshot" class="text-xs text-[var(--mn-ink-muted)]">{{ t("已保留 {value} 条系统防护规则，优先于用户业务规则执行。", { value: snapshot.protectedCount }) }}</p>
      <p v-if="status" role="status" class="text-sm">{{ status }}</p>
      <p v-if="error" role="alert" class="text-sm text-[var(--mn-danger)]">{{ error }}</p>
      <template v-if="snapshot">
        <div class="routing-policy-toolbar">
          <label class="grid gap-1"><span class="text-sm">{{ t("搜索路由规则") }}</span><input v-model="query" class="routing-rule-input" type="search" /></label>
          <Button size="sm" :disabled="mutationLocked" @click="editRule(null)"><Plus :size="15" />{{ t("添加路由规则") }}</Button>
        </div>
        <div v-if="ruleDraft" class="routing-rule-editor">
          <label class="grid gap-1"><span>{{ t("匹配条件（JSON）") }}</span><Textarea v-model="ruleDraft.conditions" :rows="6" spellcheck="false" /></label>
          <p class="text-xs text-[var(--mn-ink-muted)]">{{ t("支持域名、规则集、IP、端口、协议、包名以及逻辑组合；修改仅在保存并应用后生效。") }}</p>
          <label class="grid gap-1"><span>{{ t("目标出站") }}</span><select v-model="ruleDraft.target" class="routing-rule-input"><option v-if="!targets.some(target => target.tag === ruleDraft?.target)" :value="ruleDraft.target">{{ ruleDraft.target }} · {{ t("不可用") }}</option><option v-for="target in targets" :key="target.tag" :value="target.tag">{{ target.tag }} · {{ target.type }}</option></select></label>
          <div class="flex gap-2"><Button size="sm" variant="outline" @click="ruleDraft = null">{{ t("取消") }}</Button><Button size="sm" @click="keepRule">{{ t("保留规则修改") }}</Button></div>
        </div>
        <div class="overflow-x-auto">
          <table class="routing-rule-table w-full text-sm">
            <thead><tr><th>{{ t("顺序") }}</th><th>{{ t("匹配条件") }}</th><th>{{ t("目标出站") }}</th><th>{{ t("操作") }}</th></tr></thead>
            <tbody><tr v-for="row in rows" :key="row.index" class="routing-rule-row">
              <td>{{ row.index + 1 }}</td><td class="routing-rule-condition">{{ JSON.stringify(row.rule.conditions) }}</td><td>{{ row.rule.target }}<span v-if="!targets.some(target => target.tag === row.rule.target)" class="text-[var(--mn-warning)]"> · {{ t("不可用，将阻断") }}</span></td>
              <td><div class="flex gap-1">
                <Button size="sm" variant="ghost" :disabled="mutationLocked" :aria-label="t('编辑规则 {value}', { value: row.index + 1 })" @click="editRule(row.index)"><Pencil :size="14" /></Button>
                <Button size="sm" variant="ghost" :disabled="mutationLocked || Boolean(query) || row.index === 0" :aria-label="t('上移规则 {value}', { value: row.index + 1 })" @click="move(row.index, -1)"><ArrowUp :size="14" /></Button>
                <Button size="sm" variant="ghost" :disabled="mutationLocked || Boolean(query) || row.index === policy.rules.length - 1" :aria-label="t('下移规则 {value}', { value: row.index + 1 })" @click="move(row.index, 1)"><ArrowDown :size="14" /></Button>
                <Button size="sm" variant="ghost" :disabled="mutationLocked" :aria-label="t('删除规则 {value}', { value: row.index + 1 })" @click="requestDelete('rule', row.index)"><Trash2 :size="14" /></Button>
              </div></td>
            </tr><tr v-if="!rows.length"><td colspan="4">{{ t("没有匹配的路由规则。") }}</td></tr></tbody>
          </table>
        </div>
        <label class="grid gap-1 text-sm"><span>{{ t("兜底出站") }}</span><select v-model="policy.final" class="routing-rule-input" :disabled="mutationLocked"><option v-if="!targets.some(target => target.tag === policy.final)" :value="policy.final">{{ policy.final }} · {{ t("不可用") }}</option><option v-for="target in targets.filter(target => target.tag !== 'final')" :key="target.tag" :value="target.tag">{{ target.tag }}</option></select></label>
        <CardHeading :title="t('用户策略组')"><Button size="sm" :disabled="mutationLocked" @click="editGroup(null)"><Plus :size="15" />{{ t("添加策略组") }}</Button></CardHeading>
        <p class="text-xs text-[var(--mn-ink-muted)]">{{ t("默认仅提供 proxy、proxy-auto 和 final。多条规则需要共用出口时再创建用户策略组。") }}</p>
        <div v-if="groupDraft" class="routing-rule-editor">
          <label class="grid gap-1"><span>{{ t("策略组名称") }}</span><input v-model="groupDraft.tag" class="routing-rule-input" /></label>
          <label class="grid gap-1"><span>{{ t("策略组类型") }}</span><select v-model="groupDraft.type" class="routing-rule-input"><option value="selector">{{ t("手动选择") }}</option><option value="urltest">{{ t("自动测速") }}</option></select></label>
          <label class="grid gap-1"><span>{{ t("策略组成员（可多选）") }}</span><select v-model="groupDraft.members" multiple class="routing-rule-input routing-group-members"><option v-for="target in memberTargets" :key="target.tag" :value="target.tag">{{ target.tag }}</option></select></label>
          <label v-if="groupDraft.type === 'selector'" class="grid gap-1"><span>{{ t("默认出站") }}</span><select v-model="groupDraft.default" class="routing-rule-input"><option v-for="member in groupDraft.members" :key="member" :value="member">{{ member }}</option></select></label>
          <label v-else class="grid gap-1"><span>{{ t("测速地址（HTTPS）") }}</span><input v-model="groupDraft.url" class="routing-rule-input" type="url" /></label>
          <div class="flex gap-2"><Button size="sm" variant="outline" @click="groupDraft = null">{{ t("取消") }}</Button><Button size="sm" @click="keepGroup">{{ t("保留策略组修改") }}</Button></div>
        </div>
        <div v-for="(group, index) in policy.groups" :key="group.tag" class="routing-user-group">
          <div><strong>{{ group.tag }}</strong><p class="text-xs">{{ group.type }} · {{ group.members.join(' / ') }}</p></div>
          <div class="flex gap-1"><Button size="sm" variant="ghost" :disabled="mutationLocked" :aria-label="t('编辑策略组 {value}', { value: group.tag })" @click="editGroup(index)"><Pencil :size="14" /></Button><Button size="sm" variant="ghost" :disabled="mutationLocked || routingGroupReferenced(policy, group.tag)" :aria-label="t('删除策略组 {value}', { value: group.tag })" :title="routingGroupReferenced(policy, group.tag) ? t('策略组仍被引用，请先修改规则、成员或兜底出站。') : undefined" @click="requestDelete('group', index)"><Trash2 :size="14" /></Button></div>
        </div>
        <p v-if="!policy.groups.length" class="text-sm text-[var(--mn-ink-muted)]">{{ t("尚未创建用户策略组。") }}</p>
        <div v-if="changed" class="routing-policy-toolbar"><p class="text-sm">{{ t("修改尚未保存。") }}</p><div class="flex gap-2"><Button size="sm" variant="outline" :disabled="busy || Boolean(pendingApply || pendingDelete)" @click="discard">{{ t("放弃修改") }}</Button><Button size="sm" :disabled="mutationLocked" @click="requestApply">{{ t("保存并应用") }}</Button></div></div>
      </template>
      <ConfirmPanel v-if="pendingDelete" :title="t('删除路由配置条目')" :detail="t('删除先保留在编辑器中，保存并应用后才会改变设备配置。')" :loading="busy" :confirm-label="t('确认删除')" confirm-variant="destructive" @cancel="pendingDelete = null" @confirm="deleteEntry" />
      <ConfirmPanel v-if="pendingApply" :title="t('应用路由策略')" :detail="t('将校验并保存规则、用户策略组和兜底出站；核心可能重启，失败时恢复之前的配置。')" :loading="busy" :confirm-label="t('确认应用')" @cancel="pendingApply = false" @confirm="apply" />
    </Card>
  </div>
</template>

<style scoped>
.routing-policy-toolbar, .routing-user-group { display: flex; flex-wrap: wrap; justify-content: space-between; align-items: center; gap: 10px; }
.routing-rule-editor { display: grid; gap: 10px; border: 1px solid var(--mn-border); border-radius: 10px; padding: 12px; background: var(--mn-surface-sunken); }
.routing-rule-input { min-width: 0; width: 100%; border: 1px solid var(--mn-border); border-radius: 7px; padding: 7px 9px; color: var(--mn-ink); background: var(--mn-surface); }
.routing-group-members { min-height: 120px; }
.routing-rule-table th, .routing-rule-table td { padding: 8px 9px; border-bottom: 1px solid var(--mn-border); text-align: left; vertical-align: middle; }
.routing-rule-table th { color: var(--mn-ink-muted); font-size: 12px; font-weight: 600; white-space: nowrap; }
.routing-rule-condition { min-width: 200px; max-width: 520px; overflow-wrap: anywhere; }
.routing-user-group { border-bottom: 1px solid var(--mn-border); padding: 9px 0; overflow-wrap: anywhere; }
</style>
