# 贡献指南

感谢你考虑为智研做出贡献！本文档将帮助你了解如何参与开发。

## 开发环境搭建

### 前置要求

- **Node.js** ≥ 18
- **Rust** 工具链（`stable-x86_64-pc-windows-msvc`）— 通过 [rustup](https://rustup.rs/) 安装
- **Microsoft Visual Studio C++ Build Tools**（安装时勾选"使用 C++ 的桌面开发"）
- **Windows SDK**（通常随 Build Tools 一起安装）
- **WebView2 Runtime**（Windows 11 自带）

### 启动步骤

```bash
# 1. Fork 并克隆仓库
git clone https://github.com/Suxiao-yong/zhiyan.git
cd zhiyan

# 2. 安装依赖
npm install

# 3. 启动开发模式（前端热重载 + Rust 自动编译）
npm run tauri dev

# 4. 运行测试
npm run test

# 5. 类型检查
npm run typecheck
```

## 代码规范

- **TypeScript**：strict 模式，所有新代码必须有类型标注
- **Vue**：使用 Composition API + `<script setup lang="ts">`
- **Pinia stores**：业务 store 不持久化（避免敏感数据落入 localStorage）
- **ESLint + Prettier**：提交前运行 `npm run lint` 和 `npm run format`
- **CSS**：优先使用 Tailwind CSS utility classes，复杂样式用 scoped CSS

```bash
npm run lint       # ESLint 自动修复
npm run format     # Prettier 格式化
```

## 项目结构

```
src/
├── components/    # Vue 组件（按功能模块分目录）
├── pages/         # 页面组件（路由入口）
├── services/      # 业务逻辑（llm-adapter / search / analyzer / db）
├── stores/        # Pinia 状态管理
├── router/        # Vue Router
├── types/         # TypeScript 类型定义
└── assets/        # 静态资源

src-tauri/src/     # Rust 后端（credentials / db / lib / main）
```

### 添加新页面

1. 在 `src/pages/` 创建 Vue SFC
2. 在 `src/router/index.ts` 注册路由
3. 如需新的 store，在 `src/stores/` 创建
4. 如需新的 service，在 `src/services/` 创建

### 添加新组件

放在对应的功能目录下（如 `components/plan/`），保持单一职责。

## Pull Request 流程

1. 从 `main` 分支创建特性分支：`git checkout -b feat/your-feature`
2. 编写代码 + 测试
3. 确保 `npm run test` 和 `npm run typecheck` 通过
4. 提交 commit（建议使用 [Conventional Commits](https://www.conventionalcommits.org/)）
5. Push 到你的 fork 并提交 PR

### PR 检查清单

- [ ] 代码通过 ESLint 和 TypeScript 检查
- [ ] 新功能有对应的测试
- [ ] 如有 UI 变更，附截图
- [ ] commit message 清晰描述改动

## Issue 提交

- **Bug**：使用 Bug Report 模板，提供复现步骤和环境信息
- **功能建议**：使用 Feature Request 模板，描述问题和期望方案

## 行为准则

本项目遵循 [Contributor Covenant v2.1](./CODE_OF_CONDUCT.md)。请保持友善和尊重的交流。
