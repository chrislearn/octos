import { useState } from 'react'
import type { ProfileConfig } from '../types'
import LlmProviderTab from './tabs/LlmProviderTab'
import SearchApiTab from './tabs/SearchApiTab'
import PptConfigTab from './tabs/PptConfigTab'
import TelegramTab from './tabs/TelegramTab'
import WhatsAppTab from './tabs/WhatsAppTab'
import FeishuTab from './tabs/FeishuTab'
import GatewayTab from './tabs/GatewayTab'

interface Props {
  initialId?: string
  initialName?: string
  initialEnabled?: boolean
  initialConfig?: ProfileConfig
  isNew?: boolean
  onSubmit: (data: {
    id: string
    name: string
    enabled: boolean
    config: ProfileConfig
  }) => void
  onCancel: () => void
  loading?: boolean
}

const defaultConfig: ProfileConfig = {
  provider: 'anthropic',
  model: 'claude-sonnet-4-20250514',
  api_key_env: 'ANTHROPIC_API_KEY',
  channels: [],
  gateway: { max_history: 50 },
  env_vars: {},
}

type Tab = 'general' | 'llm' | 'search' | 'ppt' | 'telegram' | 'whatsapp' | 'feishu' | 'gateway' | 'env'

export default function ProfileForm({
  initialId = '',
  initialName = '',
  initialEnabled = true,
  initialConfig,
  isNew = false,
  onSubmit,
  onCancel,
  loading = false,
}: Props) {
  const [id, setId] = useState(initialId)
  const [name, setName] = useState(initialName)
  const [enabled, setEnabled] = useState(initialEnabled)
  const [config, setConfig] = useState<ProfileConfig>(initialConfig || defaultConfig)
  const [activeTab, setActiveTab] = useState<Tab>('general')

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault()
    onSubmit({ id, name, enabled, config })
  }

  const addEnvVar = () => {
    setConfig({ ...config, env_vars: { ...config.env_vars, '': '' } })
  }

  const removeEnvVar = (key: string) => {
    const { [key]: _, ...rest } = config.env_vars
    setConfig({ ...config, env_vars: rest })
  }

  const updateEnvVar = (oldKey: string, newKey: string, value: string) => {
    const entries = Object.entries(config.env_vars).map(([k, v]) =>
      k === oldKey ? [newKey, value] : [k, v],
    )
    setConfig({ ...config, env_vars: Object.fromEntries(entries) })
  }

  const tabs: { key: Tab; label: string }[] = [
    { key: 'general', label: 'General' },
    { key: 'llm', label: 'LLM Provider' },
    { key: 'search', label: 'Search APIs' },
    { key: 'ppt', label: 'PPT' },
    { key: 'telegram', label: 'Telegram' },
    { key: 'whatsapp', label: 'WhatsApp' },
    { key: 'feishu', label: 'Feishu' },
    { key: 'gateway', label: 'Gateway' },
    { key: 'env', label: 'Env Vars' },
  ]

  return (
    <form onSubmit={handleSubmit}>
      {/* Tab navigation */}
      <div className="flex border-b border-gray-700/50 mb-6 overflow-x-auto">
        {tabs.map((tab) => (
          <button
            key={tab.key}
            type="button"
            onClick={() => setActiveTab(tab.key)}
            className={`px-4 py-2.5 text-sm font-medium border-b-2 transition whitespace-nowrap ${
              activeTab === tab.key
                ? 'border-accent text-accent'
                : 'border-transparent text-gray-500 hover:text-gray-300'
            }`}
          >
            {tab.label}
          </button>
        ))}
      </div>

      {/* General Tab */}
      {activeTab === 'general' && (
        <div className="space-y-4">
          <Field label="Profile ID" hint="Lowercase letters, digits, hyphens. Cannot change after creation.">
            <input
              value={id}
              onChange={(e) => setId(e.target.value.toLowerCase().replace(/[^a-z0-9-]/g, ''))}
              disabled={!isNew}
              placeholder="alice-bot"
              className="input"
              required
            />
          </Field>
          <Field label="Display Name">
            <input
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder="Alice's Bot"
              className="input"
              required
            />
          </Field>
          <Field label="Auto-start">
            <label className="flex items-center gap-2 cursor-pointer">
              <input
                type="checkbox"
                checked={enabled}
                onChange={(e) => setEnabled(e.target.checked)}
                className="w-4 h-4 rounded bg-surface-dark border-gray-600 text-accent focus:ring-accent"
              />
              <span className="text-sm text-gray-400">Start gateway automatically when server starts</span>
            </label>
          </Field>
        </div>
      )}

      {/* Config tab sub-components */}
      {activeTab === 'llm' && <LlmProviderTab config={config} onChange={setConfig} />}
      {activeTab === 'search' && <SearchApiTab config={config} onChange={setConfig} />}
      {activeTab === 'ppt' && <PptConfigTab config={config} onChange={setConfig} />}
      {activeTab === 'telegram' && <TelegramTab config={config} onChange={setConfig} />}
      {activeTab === 'whatsapp' && <WhatsAppTab config={config} onChange={setConfig} />}
      {activeTab === 'feishu' && <FeishuTab config={config} onChange={setConfig} />}
      {activeTab === 'gateway' && <GatewayTab config={config} onChange={setConfig} />}

      {/* Env Vars Tab (raw key-value editor for admin) */}
      {activeTab === 'env' && (
        <div className="space-y-4">
          <p className="text-xs text-gray-500">
            Raw environment variables passed to the gateway process. API keys configured in other tabs
            appear here automatically.
          </p>

          {Object.entries(config.env_vars).map(([key, value], i) => (
            <div key={i} className="flex gap-2 items-start">
              <div className="flex-1">
                <input
                  value={key}
                  onChange={(e) => updateEnvVar(key, e.target.value, value)}
                  placeholder="ANTHROPIC_API_KEY"
                  className="input text-xs font-mono"
                />
              </div>
              <div className="flex-[2]">
                <input
                  type="password"
                  value={value}
                  onChange={(e) => updateEnvVar(key, key, e.target.value)}
                  placeholder="sk-ant-..."
                  className="input text-xs font-mono"
                />
              </div>
              <button
                type="button"
                onClick={() => removeEnvVar(key)}
                className="px-2 py-2 text-xs text-red-400 hover:text-red-300"
              >
                <svg className="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                  <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M6 18L18 6M6 6l12 12" />
                </svg>
              </button>
            </div>
          ))}

          <button
            type="button"
            onClick={addEnvVar}
            className="px-3 py-1.5 text-xs font-medium rounded-lg bg-white/5 text-gray-400 hover:bg-white/10 hover:text-white border border-gray-700/50 transition"
          >
            + Add Environment Variable
          </button>
        </div>
      )}

      {/* Actions */}
      <div className="flex justify-end gap-3 mt-8 pt-4 border-t border-gray-700/50">
        <button
          type="button"
          onClick={onCancel}
          className="px-4 py-2 text-sm font-medium text-gray-400 hover:text-white rounded-lg hover:bg-white/5 transition"
        >
          Cancel
        </button>
        <button
          type="submit"
          disabled={loading}
          className="px-6 py-2 text-sm font-medium rounded-lg bg-accent text-white hover:bg-accent-light transition disabled:opacity-50"
        >
          {loading ? 'Saving...' : isNew ? 'Create Profile' : 'Save Changes'}
        </button>
      </div>
    </form>
  )
}

function Field({
  label,
  hint,
  children,
}: {
  label: string
  hint?: string
  children: React.ReactNode
}) {
  return (
    <div>
      <label className="block text-sm font-medium text-gray-300 mb-1.5">{label}</label>
      {hint && <p className="text-xs text-gray-500 mb-1.5">{hint}</p>}
      {children}
    </div>
  )
}
