/**
 * 启动 boot 副作用：必须是 main.tsx 的首个 import。
 * 在任何 store 模块创建（analysisSettings 同步水合）之前，
 * 把上次会话导入的配置写回 localStorage。
 */
import { applyPendingConfigImport } from "@/lib/configTransfer";

applyPendingConfigImport();
