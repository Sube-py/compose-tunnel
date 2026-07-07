<script setup lang="ts">
import { computed, onMounted, reactive, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { useConfirm } from "primevue/useconfirm";
import { useToast } from "primevue/usetoast";
import Button from "primevue/button";
import Card from "primevue/card";
import Column from "primevue/column";
import ConfirmDialog from "primevue/confirmdialog";
import DataTable from "primevue/datatable";
import Dialog from "primevue/dialog";
import InputNumber from "primevue/inputnumber";
import InputText from "primevue/inputtext";
import ScrollPanel from "primevue/scrollpanel";
import Select from "primevue/select";
import Tag from "primevue/tag";
import Toast from "primevue/toast";
import logoUrl from "../assets/logo.svg";

type Defaults = {
  local_host: string;
  socat_image: string;
  socat_command: string;
  ssh_binary: string;
  docker_timeout_secs: number;
};

type ServerConfig = {
  name: string;
  host: string;
  port: number;
  user: string;
  identity_file?: string | null;
  ssh_alias?: string | null;
  default_socat_image?: string | null;
  docker_command: string;
};

type ComposeProject = {
  server: string;
  project: string;
  services: string[];
};

type ComposeService = {
  service: string;
  container: string;
  status: string;
  ports: string[];
  networks: string[];
  image: string;
};

type TunnelState = {
  id: string;
  server: string;
  project: string;
  service: string;
  network: string;
  target_port: number;
  socat_port: number;
  local_host: string;
  local_port: number;
  socat_container: string;
  socat_container_ip: string;
  ssh_pid?: number | null;
  status: "running" | "stopped" | "error";
  mode: "socat-direct";
  started_at?: string | null;
  last_error?: string | null;
};

type EnvTunnelPort = {
  tunnel_id: string;
  alias: string;
  env_key?: string | null;
};

type EnvPlainEntry = {
  key: string;
  value: string;
};

type EnvProfileConfig = {
  name: string;
  target_dir?: string | null;
  tunnel_ports: EnvTunnelPort[];
  extra_env: EnvPlainEntry[];
};

type EnvProjectRow = {
  target_dir: string;
  label: string;
  active_name: string;
  selected_name: string;
  profile_count: number;
};

type CleanupResult = {
  server: string;
  containers: string[];
};

const tabs = ["Dashboard", "Servers", "Compose", "Tunnels", "Env", "Logs", "Settings"] as const;
type Tab = (typeof tabs)[number];

const toast = useToast();
const confirm = useConfirm();
const activeTab = ref<Tab>("Dashboard");
const loading = ref(false);
const logs = ref<string[]>([]);
const defaults = reactive<Defaults>({
  local_host: "127.0.0.1",
  socat_image: "alpine/socat:latest",
  socat_command: "socat",
  ssh_binary: "ssh",
  docker_timeout_secs: 20,
});
const servers = ref<ServerConfig[]>([]);
const projects = ref<ComposeProject[]>([]);
const services = ref<ComposeService[]>([]);
const tunnels = ref<TunnelState[]>([]);
const envProfiles = ref<EnvProfileConfig[]>([]);
const activeEnvProfiles = ref<Record<string, string>>({});
const selectedEnvNames = ref<Record<string, string>>({});
const selectedServer = ref("");
const selectedProject = ref("");
const envPreview = ref("");
const editingServerName = ref("");
const serverDialogVisible = ref(false);
const tunnelDialogVisible = ref(false);
const selectedEnvTargetDir = ref("");
const selectedEnvProfileName = ref("");
const editingEnvProfileName = ref("");
const envDialogVisible = ref(false);

const serverForm = reactive<ServerConfig>({
  name: "",
  host: "",
  port: 22,
  user: "",
  identity_file: "",
  ssh_alias: "",
  default_socat_image: "",
  docker_command: "docker",
});

const tunnelForm = reactive({
  server: "",
  project: "",
  service: "",
  target_port: 5432,
  network: "",
  local_port: "",
  socat_image: "",
});

const envProfileForm = reactive<EnvProfileConfig>({
  name: "",
  target_dir: "",
  tunnel_ports: [],
  extra_env: [],
});

const runningCount = computed(() => tunnels.value.filter((item) => item.status === "running").length);
const stoppedCount = computed(() => tunnels.value.filter((item) => item.status !== "running").length);
const dockerCommandPreset = computed(() => {
  if (serverForm.docker_command === "docker") {
    return "docker";
  }
  if (serverForm.docker_command === "sudo -n docker") {
    return "sudo -n docker";
  }
  return "custom";
});
const dockerModeOptions = [
  { label: "docker", value: "docker" },
  { label: "sudo -n docker", value: "sudo -n docker" },
  { label: "custom", value: "custom" },
];
const serverOptions = computed(() => servers.value.map((server) => ({ label: server.name, value: server.name })));
const envTunnelOptions = computed(() =>
  tunnels.value
    .map((tunnel) => ({
      label: `${tunnel.server}/${tunnel.project}/${tunnel.service}:${tunnel.target_port} (${tunnel.status}, ${tunnel.local_host}:${tunnel.local_port})`,
      value: tunnel.id,
    })),
);
const envProjectRows = computed<EnvProjectRow[]>(() => {
  const projects = new Map<string, EnvProjectRow>();
  for (const profile of envProfiles.value) {
    const targetDir = envTargetKey(profile);
    if (!targetDir) {
      continue;
    }
    const existing = projects.get(targetDir);
    if (existing) {
      existing.profile_count += 1;
    } else {
      projects.set(targetDir, {
        target_dir: targetDir,
        label: projectFolderName(targetDir),
        active_name: activeEnvProfiles.value[targetDir] ?? "",
        selected_name: selectedEnvNames.value[targetDir] ?? activeEnvProfiles.value[targetDir] ?? "",
        profile_count: 1,
      });
    }
  }
  if (selectedEnvTargetDir.value && !projects.has(selectedEnvTargetDir.value)) {
    projects.set(selectedEnvTargetDir.value, {
      target_dir: selectedEnvTargetDir.value,
      label: projectFolderName(selectedEnvTargetDir.value),
      active_name: activeEnvProfiles.value[selectedEnvTargetDir.value] ?? "",
      selected_name: selectedEnvNames.value[selectedEnvTargetDir.value] ?? activeEnvProfiles.value[selectedEnvTargetDir.value] ?? "",
      profile_count: 0,
    });
  }
  return Array.from(projects.values()).sort((left, right) => left.label.localeCompare(right.label));
});
const envDialogProfileOptions = computed(() => {
  const target = envProfileForm.target_dir?.trim() || selectedEnvTargetDir.value;
  return envProfiles.value
    .filter((profile) => envTargetKey(profile) === target)
    .sort((left, right) => left.name.localeCompare(right.name))
    .map((profile) => ({ label: profile.name, value: profile.name }));
});
const tunnelProjectOptions = computed(() =>
  projects.value.filter((project) => project.server === tunnelForm.server),
);
const tunnelServiceOptions = computed(() =>
  services.value.filter(() => selectedServer.value === tunnelForm.server && selectedProject.value === tunnelForm.project),
);
const selectedTunnelService = computed(
  () => tunnelServiceOptions.value.find((service) => service.service === tunnelForm.service) ?? null,
);

function setTab(tab: Tab) {
  activeTab.value = tab;
}

function log(message: string) {
  logs.value.unshift(`${new Date().toLocaleTimeString()} ${message}`);
  logs.value = logs.value.slice(0, 80);
}

async function runTask<T>(message: string, task: () => Promise<T>, showSuccessToast = true): Promise<T | null> {
  loading.value = true;
  try {
    const result = await task();
    log(message);
    if (showSuccessToast) {
      toast.add({ severity: "success", summary: message, life: 2600 });
    }
    return result;
  } catch (caught) {
    const text = caught instanceof Error ? caught.message : String(caught);
    log(`Error: ${text}`);
    toast.add({ severity: "error", summary: "Operation failed", detail: text, life: 5000 });
    return null;
  } finally {
    loading.value = false;
  }
}

async function bootstrap() {
  await runTask("Workspace loaded", async () => {
    await invoke("init_config");
    const config = await invoke<{ defaults: Defaults }>("get_config");
    Object.assign(defaults, config.defaults);
    await refreshServers();
    await refreshTunnels();
    await refreshEnvProfiles();
  });
}

async function refreshServers() {
  servers.value = await invoke<ServerConfig[]>("list_servers");
  if (!selectedServer.value && servers.value.length > 0) {
    selectedServer.value = servers.value[0].name;
  }
  if (!tunnelForm.server && servers.value.length > 0) {
    tunnelForm.server = servers.value[0].name;
  }
}

async function refreshTunnels() {
  tunnels.value = await invoke<TunnelState[]>("list_tunnels");
}

async function refreshEnvProfiles() {
  envProfiles.value = await invoke<EnvProfileConfig[]>("list_env_profiles");
  activeEnvProfiles.value = await invoke<Record<string, string>>("active_env_profiles");
  for (const profile of envProfiles.value) {
    const target = envTargetKey(profile);
    if (target && !selectedEnvNames.value[target]) {
      selectedEnvNames.value[target] = activeEnvProfiles.value[target] ?? profile.name;
    }
  }
  if (!selectedEnvTargetDir.value && envProfiles.value.length > 0) {
    selectedEnvTargetDir.value = envTargetKey(envProfiles.value[0]);
  }
  if (selectedEnvProfileName.value && envProfiles.value.some((profile) => profile.name === selectedEnvProfileName.value)) {
    loadEnvProfile(selectedEnvProfileName.value);
  }
}

async function saveServer() {
  await runTask(`Saved server ${serverForm.name}`, async () => {
    const server = compactServer(serverForm);
    if (editingServerName.value && editingServerName.value !== server.name) {
      await invoke("delete_server", { name: editingServerName.value });
    }
    await invoke("save_server", { server });
    clearServerForm();
    serverDialogVisible.value = false;
    await refreshServers();
  });
}

async function deleteServer(name: string) {
  confirm.require({
    header: "Delete server",
    message: `Delete server ${name}? Existing tunnels and env profiles that reference this server will no longer be usable until updated.`,
    icon: "pi pi-exclamation-triangle",
    rejectProps: {
      label: "Cancel",
      severity: "secondary",
      outlined: true,
    },
    acceptProps: {
      label: "Delete",
      severity: "danger",
    },
    accept: () => {
      void runDeleteServer(name);
    },
  });
}

async function runDeleteServer(name: string) {
  await runTask(`Deleted server ${name}`, async () => {
    await invoke("delete_server", { name });
    await refreshServers();
  });
}

async function testServer(name: string) {
  const result = await runTask(`Tested server ${name}`, async () =>
    invoke<{ details: string[] }>("test_server", { serverId: name }),
    false,
  );
  if (result) {
    logs.value.unshift(...result.details.map((detail) => `${new Date().toLocaleTimeString()} ${detail}`));
    const ok = result.details.length > 0 && !result.details.some((detail) => detail.toLowerCase().includes("failed"));
    toast.add({
      severity: ok ? "success" : "error",
      summary: ok ? "Connection test passed" : "Connection test failed",
      detail: result.details.join(" | "),
      life: ok ? 3500 : 7000,
    });
  }
}

function editServer(server: ServerConfig) {
  editingServerName.value = server.name;
  Object.assign(serverForm, {
    name: server.name,
    host: server.host,
    port: server.port,
    user: server.user,
    identity_file: server.identity_file ?? "",
    ssh_alias: server.ssh_alias ?? "",
    default_socat_image: server.default_socat_image ?? "",
    docker_command: server.docker_command || "docker",
  });
  serverDialogVisible.value = true;
}

function openNewServerDialog() {
  clearServerForm();
  serverDialogVisible.value = true;
}

function applyDockerCommandPreset(value: string) {
  if (value === "docker" || value === "sudo -n docker") {
    serverForm.docker_command = value;
  }
}

async function discoverProjects() {
  if (!selectedServer.value) {
    toast.add({ severity: "warn", summary: "Select a server first", life: 3000 });
    return;
  }
  const result = await runTask(`Discovered projects on ${selectedServer.value}`, async () =>
    invoke<ComposeProject[]>("list_compose_projects", { serverId: selectedServer.value }),
  );
  if (result) {
    projects.value = result;
    services.value = [];
    selectedProject.value = "";
  }
}

async function loadServices(project: string) {
  selectedProject.value = project;
  const result = await runTask(`Loaded services for ${project}`, async () =>
    invoke<ComposeService[]>("list_compose_services", {
      serverId: selectedServer.value,
      project,
    }),
  );
  if (result) {
    services.value = result;
  }
}

function pickService(service: ComposeService) {
  tunnelForm.server = selectedServer.value;
  tunnelForm.project = selectedProject.value;
  tunnelForm.service = service.service;
  tunnelForm.network = service.networks[0] ?? "";
  tunnelForm.local_port = "";
  const port = inferPort(service);
  if (port) {
    tunnelForm.target_port = port;
  }
  activeTab.value = "Tunnels";
  tunnelDialogVisible.value = true;
}

async function openTunnel() {
  await runTask(`Opened tunnel for ${tunnelForm.service}`, async () => {
    await invoke("open_tunnel", {
      request: {
        server: tunnelForm.server,
        project: tunnelForm.project,
        service: tunnelForm.service,
        target_port: Number(tunnelForm.target_port),
        network: optional(tunnelForm.network),
        local_port: tunnelForm.local_port ? Number(tunnelForm.local_port) : null,
        local_host: defaults.local_host,
        socat_port: null,
        socat_image: optional(tunnelForm.socat_image),
      },
    });
    tunnelForm.local_port = "";
    tunnelDialogVisible.value = false;
    await refreshTunnels();
  });
}

function openTunnelDialog() {
  tunnelDialogVisible.value = true;
}

async function onTunnelServerChange() {
  selectedServer.value = tunnelForm.server;
  tunnelForm.project = "";
  tunnelForm.service = "";
  tunnelForm.network = "";
  services.value = [];
  if (tunnelForm.server) {
    const result = await runTask(`Discovered projects on ${tunnelForm.server}`, async () =>
      invoke<ComposeProject[]>("list_compose_projects", { serverId: tunnelForm.server }),
    );
    if (result) {
      projects.value = result;
    }
  }
}

async function onTunnelProjectChange() {
  selectedServer.value = tunnelForm.server;
  selectedProject.value = tunnelForm.project;
  tunnelForm.service = "";
  tunnelForm.network = "";
  if (tunnelForm.server && tunnelForm.project) {
    await loadServices(tunnelForm.project);
  }
}

function onTunnelServiceChange() {
  const service = selectedTunnelService.value;
  if (!service) {
    tunnelForm.network = "";
    return;
  }
  tunnelForm.network = service.networks[0] ?? "";
  const port = inferPort(service);
  if (port) {
    tunnelForm.target_port = port;
  }
}

async function closeTunnel(id: string) {
  await runTask(`Stopped tunnel ${id}`, async () => {
    await invoke("close_tunnel", { tunnelId: id });
    await refreshTunnels();
  });
}

async function stopAllTunnels() {
  confirm.require({
    header: "Stop all tunnels",
    message: `Stop ${runningCount.value} running tunnel(s) and remove their remote socat containers?`,
    icon: "pi pi-exclamation-triangle",
    rejectProps: {
      label: "Cancel",
      severity: "secondary",
      outlined: true,
    },
    acceptProps: {
      label: "Stop All",
      severity: "danger",
    },
    accept: () => {
      void runStopAllTunnels();
    },
  });
}

async function runStopAllTunnels() {
  await runTask("Stopped all tunnels", async () => {
    await invoke("close_all_tunnels");
    await refreshTunnels();
  });
}

async function cleanupSelectedServer() {
  if (!selectedServer.value) {
    toast.add({ severity: "warn", summary: "Select a server first", life: 3000 });
    return;
  }
  const preview = await runTask(
    `Previewed cleanup ${selectedServer.value}`,
    async () => invoke<CleanupResult>("preview_cleanup", { serverId: selectedServer.value }),
    false,
  );
  if (!preview) {
    return;
  }
  if (preview.containers.length === 0) {
    toast.add({ severity: "info", summary: "Nothing to clean up", detail: `No containers found on ${preview.server}`, life: 3200 });
    return;
  }

  const containerList = preview.containers.slice(0, 5).join(", ");
  const extraCount = preview.containers.length > 5 ? ` and ${preview.containers.length - 5} more` : "";
  confirm.require({
    header: "Cleanup remote containers",
    message: `Remove ${preview.containers.length} compose-tunnel container(s) from ${preview.server}: ${containerList}${extraCount}?`,
    icon: "pi pi-exclamation-triangle",
    rejectProps: {
      label: "Cancel",
      severity: "secondary",
      outlined: true,
    },
    acceptProps: {
      label: "Cleanup",
      severity: "danger",
    },
    accept: () => {
      void runCleanupSelectedServer();
    },
  });
}

async function runCleanupSelectedServer() {
  const result = await runTask(
    `Cleaned up ${selectedServer.value}`,
    async () => invoke<CleanupResult>("cleanup", { serverId: selectedServer.value }),
    false,
  );
  if (!result) {
    return;
  }
  const detail = result.containers.length
    ? `${result.containers.length} containers removed`
    : "No compose-tunnel containers found";
  logs.value.unshift(
    `${new Date().toLocaleTimeString()} Cleanup ${result.server}: ${result.containers.join(", ") || "nothing to remove"}`,
  );
  toast.add({ severity: "success", summary: "Cleanup complete", detail, life: 3600 });
  await refreshTunnels();
}

async function startTunnel(tunnel: TunnelState) {
  await runTask(`Started tunnel ${tunnel.id}`, async () => {
    await openTunnelFromState(tunnel, null);
    await refreshTunnels();
  });
}

async function restartTunnel(tunnel: TunnelState) {
  await runTask(`Restarted tunnel ${tunnel.id}`, async () => {
    await invoke("close_tunnel", { tunnelId: tunnel.id });
    await openTunnelFromState(tunnel, tunnel.local_port);
    await refreshTunnels();
  });
}

async function openTunnelFromState(tunnel: TunnelState, localPort: number | null) {
  await invoke("open_tunnel", {
    request: {
      server: tunnel.server,
      project: tunnel.project,
      service: tunnel.service,
      target_port: tunnel.target_port,
      network: tunnel.network,
      local_port: localPort,
      local_host: tunnel.local_host || defaults.local_host,
      socat_port: tunnel.socat_port,
      socat_image: null,
    },
  });
}

async function saveDefaultSettings() {
  await runTask("Saved settings", async () => {
    await invoke("save_defaults", { defaults: { ...defaults } });
  });
}

function newEnvProfile() {
  selectedEnvProfileName.value = "";
  editingEnvProfileName.value = "";
  envPreview.value = "";
  Object.assign(envProfileForm, {
    name: "",
    target_dir: selectedEnvTargetDir.value,
    tunnel_ports: [],
    extra_env: [],
  });
}

function openNewEnvDialog() {
  if (!selectedEnvTargetDir.value) {
    toast.add({ severity: "warn", summary: "Choose a project first", life: 3000 });
    return;
  }
  newEnvProfile();
  envDialogVisible.value = true;
}

function newEnvProfileInDialog() {
  const target = envProfileForm.target_dir?.trim() || selectedEnvTargetDir.value;
  selectedEnvTargetDir.value = target;
  selectedEnvProfileName.value = "";
  editingEnvProfileName.value = "";
  envPreview.value = "";
  Object.assign(envProfileForm, {
    name: "",
    target_dir: target,
    tunnel_ports: [],
    extra_env: [],
  });
}

function openEditEnvDialog(profile: EnvProfileConfig) {
  loadEnvProfile(profile.name);
  envDialogVisible.value = true;
}

function switchEnvProfileInDialog(name: string | null) {
  if (name) {
    loadEnvProfile(name);
  }
}

function loadEnvProfile(name: string) {
  const profile = envProfiles.value.find((item) => item.name === name);
  if (!profile) {
    newEnvProfile();
    return;
  }
  selectedEnvProfileName.value = profile.name;
  editingEnvProfileName.value = profile.name;
  envPreview.value = "";
  Object.assign(envProfileForm, {
    name: profile.name,
    target_dir: profile.target_dir ?? "",
    tunnel_ports: profile.tunnel_ports.map((item) => ({ ...item })),
    extra_env: profile.extra_env.map((item) => ({ ...item })),
  });
}

function compactEnvProfile(): EnvProfileConfig {
  return {
    name: envProfileForm.name.trim(),
    target_dir: optional(envProfileForm.target_dir),
    tunnel_ports: envProfileForm.tunnel_ports
      .filter((item) => item.tunnel_id.trim() && item.alias.trim())
      .map((item) => ({
        tunnel_id: item.tunnel_id.trim(),
        alias: item.alias.trim(),
        env_key: optional(item.env_key),
      })),
    extra_env: envProfileForm.extra_env
      .filter((item) => item.key.trim())
      .map((item) => ({ key: item.key.trim(), value: item.value })),
  };
}

async function saveEnvProfile(showToast = true, closeDialog = false) {
  if (!envProfileForm.name.trim()) {
    toast.add({ severity: "warn", summary: "Env name is required", life: 3000 });
    return false;
  }
  if (!envProfileForm.target_dir?.trim()) {
    toast.add({ severity: "warn", summary: "Target directory is required", life: 3000 });
    return false;
  }
  const profile = compactEnvProfile();
  const originalName = editingEnvProfileName.value;
  const isRename = Boolean(originalName) && originalName !== profile.name;
  const nameExists = envProfiles.value.some((item) => item.name === profile.name && item.name !== originalName);
  if (nameExists) {
    toast.add({ severity: "warn", summary: `Env ${profile.name} already exists`, life: 3000 });
    return false;
  }
  const originalProfile = originalName ? envProfiles.value.find((item) => item.name === originalName) : null;
  const wasActive = originalProfile ? isActiveEnvProfile(originalProfile) : false;
  const result = await runTask(
    `Saved env ${profile.name}`,
    async () => {
      if (isRename) {
        await invoke("delete_env_profile", { name: originalName });
      }
      await invoke("save_env_profile", { profile });
      if (wasActive) {
        await invoke("set_active_env_profile", { name: profile.name });
      }
      await refreshEnvProfiles();
      selectedEnvTargetDir.value = envTargetKey(profile);
      selectedEnvProfileName.value = profile.name;
      editingEnvProfileName.value = profile.name;
      loadEnvProfile(profile.name);
      if (closeDialog) {
        envDialogVisible.value = false;
      }
    },
    showToast,
  );
  return result !== null;
}

async function deleteEnvProfile(profile?: EnvProfileConfig) {
  const name = profile?.name ?? selectedEnvProfileName.value;
  if (!name) {
    toast.add({ severity: "warn", summary: "Select an env first", life: 3000 });
    return;
  }
  confirm.require({
    header: "Delete env profile",
    message: `Delete env profile ${name}? This removes the saved compose-tunnel env configuration for its project directory.`,
    icon: "pi pi-exclamation-triangle",
    rejectProps: {
      label: "Cancel",
      severity: "secondary",
      outlined: true,
    },
    acceptProps: {
      label: "Delete",
      severity: "danger",
    },
    accept: () => {
      void runDeleteEnvProfile(name);
    },
  });
}

async function runDeleteEnvProfile(name: string) {
  await runTask(`Deleted env ${name}`, async () => {
    await invoke("delete_env_profile", { name });
    if (selectedEnvProfileName.value === name) {
      newEnvProfile();
      envDialogVisible.value = false;
    }
    await refreshEnvProfiles();
  });
}

async function chooseEnvProject() {
  const selected = await open({ directory: true, multiple: false });
  if (typeof selected === "string") {
    selectedEnvTargetDir.value = selected;
    openNewEnvDialog();
  }
}

function profilesForProject(project: EnvProjectRow) {
  return envProfiles.value
    .filter((profile) => envTargetKey(profile) === project.target_dir)
    .sort((left, right) => left.name.localeCompare(right.name));
}

function envOptionsForProject(project: EnvProjectRow) {
  return profilesForProject(project).map((profile) => ({
    label: profile.name,
    value: profile.name,
  }));
}

function selectEnvForProject(project: EnvProjectRow, name: string | null) {
  if (name) {
    selectedEnvNames.value[project.target_dir] = name;
  }
}

function activeProfileForProject(project: EnvProjectRow) {
  const activeName = activeEnvProfiles.value[project.target_dir] ?? "";
  return envProfiles.value.find((profile) => profile.name === activeName) ?? null;
}

function selectedProfileForProject(project: EnvProjectRow) {
  const selectedName = selectedEnvNames.value[project.target_dir] || project.selected_name || project.active_name;
  return envProfiles.value.find((profile) => profile.name === selectedName) ?? activeProfileForProject(project) ?? profilesForProject(project)[0] ?? null;
}

function openEditProjectEnv(project: EnvProjectRow) {
  const profile = selectedProfileForProject(project);
  if (!profile) {
    toast.add({ severity: "warn", summary: "Add an env first", life: 3000 });
    return;
  }
  openEditEnvDialog(profile);
}

async function previewProjectEnv(project: EnvProjectRow) {
  const profile = selectedProfileForProject(project);
  if (!profile) {
    toast.add({ severity: "warn", summary: "Select an env first", life: 3000 });
    return;
  }
  await renderEnvProfilePreview(profile);
}

async function deleteActiveEnvForProject(project: EnvProjectRow) {
  const profile = selectedProfileForProject(project);
  if (!profile) {
    toast.add({ severity: "warn", summary: "Add an env first", life: 3000 });
    return;
  }
  await deleteEnvProfile(profile);
}

async function enableProjectEnv(project: EnvProjectRow) {
  const profile = selectedProfileForProject(project);
  if (!profile) {
    toast.add({ severity: "warn", summary: "Select an env first", life: 3000 });
    return;
  }
  const sensitiveKeys = sensitiveEnvKeys(profile);
  if (sensitiveKeys.length > 0) {
    confirm.require({
      header: "Enable env with sensitive values",
      message: `Start tunnels and write ${formatSensitiveEnvKeys(sensitiveKeys)} to this project's .env file?`,
      icon: "pi pi-exclamation-triangle",
      rejectProps: {
        label: "Cancel",
        severity: "secondary",
        outlined: true,
      },
      acceptProps: {
        label: "Enable",
        severity: "danger",
      },
      accept: () => {
        void runEnableProjectEnv(profile);
      },
    });
    return;
  }
  await runEnableProjectEnv(profile);
}

async function runEnableProjectEnv(profile: EnvProfileConfig) {
  await runTask(`Enabled env ${profile.name}`, async () => {
    for (const binding of profile.tunnel_ports) {
      const tunnel = tunnels.value.find((item) => item.id === binding.tunnel_id);
      if (!tunnel) {
        throw new Error(`Tunnel ${binding.tunnel_id} was not found`);
      }
      if (tunnel.status !== "running") {
        await openTunnelFromState(tunnel, null);
      }
    }
    await refreshTunnels();
    await invoke("set_active_env_profile", { name: profile.name });
    const path = await invoke<string>("write_env_profile", {
      request: { name: profile.name },
    });
    await refreshEnvProfiles();
    selectedEnvNames.value[envTargetKey(profile)] = profile.name;
    envPreview.value = await invoke<string>("render_env_profile", { name: profile.name });
    toast.add({ severity: "info", summary: "Updated .env", detail: path, life: 4500 });
  }, false);
}

function defaultPortAlias(tunnel: TunnelState) {
  return normalizeEnvName(`${tunnel.server}_${tunnel.service}`);
}

function normalizeEnvName(value: string) {
  const normalized = value.trim().replace(/[^A-Za-z0-9_]/g, "_");
  return normalized.replace(/^[0-9]/, "_$&");
}

function addEnvTunnelPortRow() {
  const tunnel = tunnels.value[0];
  envProfileForm.tunnel_ports.push({
    tunnel_id: tunnel?.id ?? "",
    alias: tunnel ? defaultPortAlias(tunnel) : "",
    env_key: "",
  });
}

function updateEnvTunnelPort(binding: EnvTunnelPort, tunnelId: string | null) {
  binding.tunnel_id = tunnelId ?? "";
  const tunnel = tunnels.value.find((item) => item.id === tunnelId);
  if (tunnel && !binding.alias.trim()) {
    binding.alias = defaultPortAlias(tunnel);
  }
}

function removeEnvTunnelPort(index: number) {
  envProfileForm.tunnel_ports.splice(index, 1);
}

function addExtraEnvRow() {
  envProfileForm.extra_env.push({ key: "", value: "" });
}

function removeExtraEnv(index: number) {
  envProfileForm.extra_env.splice(index, 1);
}

async function renderEnvProfilePreview(profile?: EnvProfileConfig) {
  let name = profile?.name ?? envProfileForm.name.trim();
  if (!profile) {
    const saved = await saveEnvProfile(false);
    if (!saved) {
      return;
    }
    name = envProfileForm.name.trim();
  }
  const result = await runTask(`Rendered env ${name}`, async () =>
    invoke<string>("render_env_profile", { name }),
  );
  if (result !== null) {
    envPreview.value = result;
  }
}

function sensitiveEnvKeys(profile: EnvProfileConfig) {
  return Array.from(
    new Set(
      profile.extra_env
        .map((entry) => entry.key.trim())
        .filter((key) => /password|token|secret|private[_-]?key/i.test(key)),
    ),
  );
}

function formatSensitiveEnvKeys(keys: string[]) {
  const visible = keys.slice(0, 5).join(", ");
  const hiddenCount = keys.length - 5;
  return hiddenCount > 0 ? `${visible}, and ${hiddenCount} more` : visible;
}

function envTargetKey(profile: EnvProfileConfig) {
  return profile.target_dir?.trim() ?? "";
}

function isActiveEnvProfile(profile: EnvProfileConfig) {
  const key = envTargetKey(profile);
  return Boolean(key) && activeEnvProfiles.value[key] === profile.name;
}

function projectFolderName(path: string) {
  const normalized = path.trim().replace(/[\\/]+$/, "");
  if (!normalized) {
    return "Project";
  }
  return normalized.split(/[\\/]/).pop() || normalized;
}

function clearServerForm() {
  editingServerName.value = "";
  Object.assign(serverForm, {
    name: "",
    host: "",
    port: 22,
    user: "",
    identity_file: "",
    ssh_alias: "",
    default_socat_image: "",
    docker_command: "docker",
  });
}

function compactServer(server: ServerConfig): ServerConfig {
  return {
    name: server.name.trim(),
    host: server.host.trim(),
    port: Number(server.port),
    user: server.user.trim(),
    identity_file: optional(server.identity_file),
    ssh_alias: optional(server.ssh_alias),
    default_socat_image: optional(server.default_socat_image),
    docker_command: server.docker_command.trim() || "docker",
  };
}

function optional(value?: string | null) {
  const trimmed = value?.trim();
  return trimmed ? trimmed : null;
}

function inferPort(service: ComposeService) {
  const text = service.ports.join(" ");
  const match = text.match(/(\d+)\/tcp/) ?? text.match(/:(\d+)->/);
  return match ? Number(match[1]) : null;
}

function statusSeverity(status: string) {
  if (status === "running") {
    return "success";
  }
  if (status === "error") {
    return "danger";
  }
  return "secondary";
}

function onDockerModeChange(value: string) {
  applyDockerCommandPreset(value);
}

onMounted(bootstrap);
</script>

<template>
  <main class="shell">
    <Toast position="top-right" />
    <ConfirmDialog />
    <aside class="sidebar">
      <div class="brand">
        <img class="brand-mark" :src="logoUrl" alt="" />
        <div>
          <h1>Compose Tunnel</h1>
          <p>SSH to Compose internals</p>
        </div>
      </div>
      <nav>
        <Button
          v-for="tab in tabs"
          :key="tab"
          :label="tab"
          :class="{ active: activeTab === tab }"
          type="button"
          text
          @click="setTab(tab)"
        />
      </nav>
    </aside>

    <ScrollPanel class="workspace-scroll">
      <section class="workspace">
        <header class="topbar">
          <div>
            <h2>{{ activeTab }}</h2>
            <p>{{ runningCount }} running tunnels, {{ stoppedCount }} stopped</p>
          </div>
          <Button label="Refresh" icon="pi pi-refresh" :loading="loading" @click="refreshTunnels" />
        </header>

      <section v-if="activeTab === 'Dashboard'" class="page">
        <div class="metrics">
          <Card>
            <template #content><span>Running</span><strong>{{ runningCount }}</strong></template>
          </Card>
          <Card>
            <template #content><span>Servers</span><strong>{{ servers.length }}</strong></template>
          </Card>
          <Card>
            <template #content><span>Projects</span><strong>{{ projects.length }}</strong></template>
          </Card>
        </div>
        <div class="toolbar">
          <Button label="Add Server" icon="pi pi-server" outlined @click="setTab('Servers')" />
          <Button label="Discover Compose" icon="pi pi-search" outlined @click="setTab('Compose')" />
          <Button label="Open Tunnel" icon="pi pi-share-alt" outlined @click="setTab('Tunnels')" />
          <Button label="Stop All" icon="pi pi-stop" severity="danger" outlined :disabled="runningCount === 0" @click="stopAllTunnels" />
        </div>
        <DataTable :value="tunnels" size="small" stripedRows>
          <Column field="id" header="ID" />
          <Column header="Service">
            <template #body="{ data }">{{ data.project }}/{{ data.service }}:{{ data.target_port }}</template>
          </Column>
          <Column header="Local">
            <template #body="{ data }">{{ data.local_host }}:{{ data.local_port }}</template>
          </Column>
          <Column header="Status">
            <template #body="{ data }"><Tag :value="data.status" :severity="statusSeverity(data.status)" /></template>
          </Column>
        </DataTable>
      </section>

      <section v-if="activeTab === 'Servers'" class="page">
        <div class="panel">
          <div class="row-between">
            <h3>Servers</h3>
            <Button label="Add Server" icon="pi pi-plus" @click="openNewServerDialog" />
          </div>
          <DataTable :value="servers" size="small" stripedRows>
            <template #empty>
              <div class="empty-state">No servers yet.</div>
            </template>
            <Column field="name" header="Name" />
            <Column header="Host">
              <template #body="{ data }">{{ data.host }}:{{ data.port }}</template>
            </Column>
            <Column field="user" header="User" />
            <Column field="docker_command" header="Docker" />
            <Column header="Actions">
              <template #body="{ data }">
                <div class="actions">
                  <Button icon="pi pi-pencil" label="Edit" size="small" text @click="editServer(data)" />
                  <Button icon="pi pi-check-circle" label="Test" size="small" text @click="testServer(data.name)" />
                  <Button icon="pi pi-trash" label="Delete" size="small" severity="danger" text @click="deleteServer(data.name)" />
                </div>
              </template>
            </Column>
          </DataTable>
        </div>

        <Dialog v-model:visible="serverDialogVisible" modal :header="editingServerName ? `Edit ${editingServerName}` : 'Add Server'" class="entity-dialog">
          <form class="form dialog-stack" @submit.prevent="saveServer">
            <div class="dialog-grid">
              <label>Name<InputText v-model="serverForm.name" required /></label>
              <label>Host<InputText v-model="serverForm.host" required /></label>
              <label>User<InputText v-model="serverForm.user" required /></label>
              <label>Port<InputNumber v-model="serverForm.port" :min="1" :useGrouping="false" fluid /></label>
              <label>Identity file<InputText v-model="serverForm.identity_file" placeholder="~/.ssh/id_ed25519" /></label>
              <label>SSH config alias<InputText v-model="serverForm.ssh_alias" placeholder="staging" /></label>
              <label>
                Docker mode
                <Select :modelValue="dockerCommandPreset" :options="dockerModeOptions" optionLabel="label" optionValue="value" @update:modelValue="onDockerModeChange" />
              </label>
              <label>Docker command<InputText v-model="serverForm.docker_command" required /></label>
              <label>Default socat image<InputText v-model="serverForm.default_socat_image" placeholder="alpine/socat:latest" /></label>
            </div>
            <div class="dialog-actions">
              <Button label="Cancel" severity="secondary" outlined type="button" @click="serverDialogVisible = false" />
              <Button :label="editingServerName ? 'Update' : 'Save'" icon="pi pi-save" type="submit" />
            </div>
          </form>
        </Dialog>
      </section>

      <section v-if="activeTab === 'Compose'" class="page">
        <div class="toolbar">
          <Select v-model="selectedServer" :options="serverOptions" optionLabel="label" optionValue="value" placeholder="Select server" />
          <Button label="Discover" icon="pi pi-search" :loading="loading" @click="discoverProjects" />
        </div>
        <div class="project-grid">
          <article v-for="project in projects" :key="project.project" class="panel">
            <div class="project-card-header">
              <h3>{{ project.project }}</h3>
              <Button label="Services" icon="pi pi-list" size="small" outlined @click="loadServices(project.project)" />
            </div>
            <p>{{ project.services.join(', ') }}</p>
          </article>
        </div>
        <DataTable :value="services" size="small" stripedRows>
          <Column field="service" header="Service" />
          <Column field="container" header="Container" />
          <Column field="status" header="Status" />
          <Column header="Ports"><template #body="{ data }">{{ data.ports.join(', ') }}</template></Column>
          <Column header="Networks"><template #body="{ data }">{{ data.networks.join(', ') }}</template></Column>
          <Column header=""><template #body="{ data }"><Button label="Tunnel" icon="pi pi-share-alt" size="small" @click="pickService(data)" /></template></Column>
        </DataTable>
      </section>

      <section v-if="activeTab === 'Tunnels'" class="page">
        <div class="panel">
          <div class="row-between">
            <h3>Tunnels</h3>
            <div class="toolbar">
              <Button label="Create Tunnel" icon="pi pi-plus" @click="openTunnelDialog" />
              <Select v-model="selectedServer" :options="serverOptions" optionLabel="label" optionValue="value" placeholder="Select server" />
              <Button label="Cleanup" icon="pi pi-trash" severity="danger" outlined @click="cleanupSelectedServer" />
            </div>
          </div>
          <DataTable :value="tunnels" size="small" stripedRows>
            <template #empty>
              <div class="empty-state">No tunnels yet.</div>
            </template>
            <Column field="id" header="ID" />
            <Column header="Remote"><template #body="{ data }">{{ data.server }} / {{ data.project }} / {{ data.service }}:{{ data.target_port }}</template></Column>
            <Column header="Local"><template #body="{ data }">{{ data.local_host }}:{{ data.local_port }}</template></Column>
            <Column header="Status"><template #body="{ data }"><Tag :value="data.status" :severity="statusSeverity(data.status)" /></template></Column>
            <Column header="">
              <template #body="{ data }">
                <div class="actions">
                  <Button
                    v-if="data.status === 'running'"
                    icon="pi pi-refresh"
                    label="Restart"
                    size="small"
                    text
                    @click="restartTunnel(data)"
                  />
                  <Button
                    v-if="data.status === 'running'"
                    label="Stop"
                    icon="pi pi-stop"
                    size="small"
                    severity="danger"
                    outlined
                    @click="closeTunnel(data.id)"
                  />
                  <Button v-else label="Start" icon="pi pi-play" size="small" severity="success" outlined @click="startTunnel(data)" />
                </div>
              </template>
            </Column>
          </DataTable>
        </div>

        <Dialog v-model:visible="tunnelDialogVisible" modal header="Create Tunnel" class="entity-dialog">
          <form class="form dialog-stack" @submit.prevent="openTunnel">
            <div class="dialog-grid">
              <label>
                Server
                <Select v-model="tunnelForm.server" :options="serverOptions" optionLabel="label" optionValue="value" placeholder="Select server" @update:modelValue="onTunnelServerChange" />
              </label>
              <label>
                Project
                <Select v-model="tunnelForm.project" :options="tunnelProjectOptions" optionLabel="project" optionValue="project" placeholder="Select project" @update:modelValue="onTunnelProjectChange" />
              </label>
              <label>
                Service container
                <Select v-model="tunnelForm.service" :options="tunnelServiceOptions" optionLabel="container" optionValue="service" placeholder="Select service" @update:modelValue="onTunnelServiceChange" />
              </label>
              <label>
                Network
                <Select v-if="selectedTunnelService" v-model="tunnelForm.network" :options="selectedTunnelService.networks" placeholder="Select network" />
                <InputText v-else v-model="tunnelForm.network" placeholder="myapp_default" />
              </label>
              <label>Target port<InputNumber v-model="tunnelForm.target_port" :min="1" :useGrouping="false" fluid /></label>
              <label>Local port<InputText v-model="tunnelForm.local_port" placeholder="auto assign" /></label>
              <label>socat image<InputText v-model="tunnelForm.socat_image" :placeholder="defaults.socat_image" /></label>
            </div>
            <div class="dialog-actions">
              <Button label="Cancel" severity="secondary" outlined type="button" @click="tunnelDialogVisible = false" />
              <Button label="Start" icon="pi pi-play" type="submit" />
            </div>
          </form>
        </Dialog>
      </section>

      <section v-if="activeTab === 'Env'" class="page">
        <div class="panel">
          <div class="row-between">
            <h3>Env Projects</h3>
            <Button label="Add Project" icon="pi pi-folder-plus" @click="chooseEnvProject" />
          </div>
          <DataTable :value="envProjectRows" size="small" stripedRows class="env-table">
            <template #empty>
              <div class="empty-state">Choose a project directory.</div>
            </template>
            <Column header="Project">
              <template #body="{ data }">
                <div class="env-project-cell">
                  <i class="pi pi-folder"></i>
                  <strong>{{ data.label }}</strong>
                </div>
              </template>
            </Column>
            <Column header="Active env">
              <template #body="{ data }">
                <Select
                  :model-value="data.selected_name"
                  :options="envOptionsForProject(data)"
                  optionLabel="label"
                  optionValue="value"
                  placeholder="Select env"
                  class="env-select"
                  @update:model-value="(name) => selectEnvForProject(data, name)"
                />
              </template>
            </Column>
            <Column header="Actions">
              <template #body="{ data }">
                <div class="actions">
                  <Button icon="pi pi-play" label="Enable" size="small" severity="success" outlined @click="enableProjectEnv(data)" />
                  <Button icon="pi pi-pencil" label="Edit" size="small" text @click="openEditProjectEnv(data)" />
                  <Button icon="pi pi-eye" label="Preview" size="small" text @click="previewProjectEnv(data)" />
                  <Button icon="pi pi-trash" label="Delete" size="small" severity="danger" text @click="deleteActiveEnvForProject(data)" />
                </div>
              </template>
            </Column>
          </DataTable>
        </div>

        <div v-if="envPreview" class="panel">
          <div class="row-between">
            <h3>.env Preview</h3>
            <Button label="Clear" icon="pi pi-times" size="small" severity="secondary" outlined @click="envPreview = ''" />
          </div>
          <pre>{{ envPreview }}</pre>
        </div>

        <Dialog v-model:visible="envDialogVisible" modal :header="`Env: ${projectFolderName(envProfileForm.target_dir || selectedEnvTargetDir)}`" class="env-dialog">
          <div class="dialog-stack form">
            <div class="env-editor-header">
              <label>
                Config
                <Select
                  :model-value="selectedEnvProfileName"
                  :options="envDialogProfileOptions"
                  optionLabel="label"
                  optionValue="value"
                  placeholder="New env"
                  @update:model-value="switchEnvProfileInDialog"
                />
              </label>
              <label>Env name<InputText v-model="envProfileForm.name" placeholder="test" /></label>
              <Button label="New Env" icon="pi pi-plus" outlined type="button" @click="newEnvProfileInDialog" />
              <Button label="Save" icon="pi pi-save" type="button" @click="saveEnvProfile(true, true)" />
            </div>

            <div class="panel nested-panel">
              <div class="env-section-header">
                <h3>Tunnel Ports</h3>
                <Button label="Add Port" icon="pi pi-plus" size="small" outlined @click="addEnvTunnelPortRow" />
              </div>
              <DataTable :value="envProfileForm.tunnel_ports" size="small" stripedRows>
                <template #empty>
                  <div class="empty-state">No tunnel ports.</div>
                </template>
                <Column header="Tunnel">
                  <template #body="{ data }">
                    <Select
                      :model-value="data.tunnel_id"
                      :options="envTunnelOptions"
                      optionLabel="label"
                      optionValue="value"
                      placeholder="Select tunnel"
                      @update:model-value="(value) => updateEnvTunnelPort(data, value)"
                    />
                  </template>
                </Column>
                <Column header="Port variable">
                  <template #body="{ data }">
                    <InputText v-model="data.alias" placeholder="server_db" />
                  </template>
                </Column>
                <Column header="Env key">
                  <template #body="{ data }">
                    <InputText v-model="data.env_key" placeholder="DATABASE_PORT" />
                  </template>
                </Column>
                <Column header="">
                  <template #body="{ index }">
                    <Button icon="pi pi-trash" label="Remove" size="small" severity="danger" text @click="removeEnvTunnelPort(index)" />
                  </template>
                </Column>
              </DataTable>
            </div>

            <div class="panel nested-panel">
              <div class="env-section-header">
                <h3>Extra Env</h3>
                <Button label="Add Variable" icon="pi pi-plus" size="small" outlined @click="addExtraEnvRow" />
              </div>
              <DataTable :value="envProfileForm.extra_env" size="small" stripedRows>
                <template #empty>
                  <div class="empty-state">No extra env variables.</div>
                </template>
                <Column header="Key">
                  <template #body="{ data }">
                    <InputText v-model="data.key" placeholder="DATABASE_HOST" />
                  </template>
                </Column>
                <Column header="Value">
                  <template #body="{ data }">
                    <InputText v-model="data.value" placeholder="127.0.0.1" />
                  </template>
                </Column>
                <Column header="">
                  <template #body="{ index }">
                    <Button icon="pi pi-trash" label="Remove" size="small" severity="danger" text @click="removeExtraEnv(index)" />
                  </template>
                </Column>
              </DataTable>
            </div>

          </div>
        </Dialog>
      </section>

      <section v-if="activeTab === 'Logs'" class="page">
        <pre>{{ logs.join('\n') }}</pre>
      </section>

      <section v-if="activeTab === 'Settings'" class="page form settings">
        <label>Default local host<InputText v-model="defaults.local_host" /></label>
        <label>Default socat image<InputText v-model="defaults.socat_image" /></label>
        <label>socat command<InputText v-model="defaults.socat_command" /></label>
        <label>SSH binary<InputText v-model="defaults.ssh_binary" /></label>
        <label>Docker timeout seconds<InputNumber v-model="defaults.docker_timeout_secs" :min="1" :useGrouping="false" fluid /></label>
        <Button label="Save Settings" icon="pi pi-save" @click="saveDefaultSettings" />
      </section>
      </section>
    </ScrollPanel>
  </main>
</template>
