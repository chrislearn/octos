import type { ProfileConfig } from '../../types'
import { PROVIDERS } from '../../types'

const PROVIDER_DEFAULTS: Record<string, { env: string; model: string }> = {
  anthropic: { env: 'ANTHROPIC_API_KEY', model: 'claude-sonnet-4-20250514' },
  openai: { env: 'OPENAI_API_KEY', model: 'gpt-4o' },
  gemini: { env: 'GEMINI_API_KEY', model: 'gemini-2.0-flash' },
  openrouter: { env: 'OPENROUTER_API_KEY', model: 'anthropic/claude-sonnet-4-20250514' },
  deepseek: { env: 'DEEPSEEK_API_KEY', model: 'deepseek-chat' },
  groq: { env: 'GROQ_API_KEY', model: 'llama-3.3-70b-versatile' },
  moonshot: { env: 'MOONSHOT_API_KEY', model: 'kimi-k2.5' },
  dashscope: { env: 'DASHSCOPE_API_KEY', model: 'qwen-max' },
  minimax: { env: 'MINIMAX_API_KEY', model: 'MiniMax-Text-01' },
  zhipu: { env: 'ZHIPU_API_KEY', model: 'glm-4-plus' },
  ollama: { env: '', model: 'llama3.2' },
  vllm: { env: 'VLLM_API_KEY', model: '' },
}

function getApiKeyEnvName(provider: string | null | undefined): string {
  return PROVIDER_DEFAULTS[provider || 'anthropic']?.env || `${(provider || 'ANTHROPIC').toUpperCase()}_API_KEY`
}

interface Props {
  config: ProfileConfig
  onChange: (config: ProfileConfig) => void
}

export default function LlmProviderTab({ config, onChange }: Props) {
  const envName = getApiKeyEnvName(config.provider)

  return (
    <div className="space-y-4">
      <div className="bg-amber-500/10 border border-amber-500/20 rounded-lg p-3 text-xs text-amber-400">
        LLM provider is required to start the gateway.
      </div>

      <Field label="Provider">
        <select
          value={config.provider || ''}
          onChange={(e) => onChange({ ...config, provider: e.target.value || null })}
          className="input"
        >
          <option value="">Auto-detect</option>
          {PROVIDERS.map((p) => (
            <option key={p} value={p}>{p}</option>
          ))}
        </select>
      </Field>

      <Field label="Model">
        <input
          value={config.model || ''}
          onChange={(e) => onChange({ ...config, model: e.target.value || null })}
          placeholder={PROVIDER_DEFAULTS[config.provider || '']?.model || 'claude-sonnet-4-20250514'}
          className="input"
        />
      </Field>

      <Field label="API Key" hint={`Stored as ${envName}`}>
        <input
          type="password"
          value={config.env_vars[envName] || ''}
          onChange={(e) => {
            const newEnvVars = { ...config.env_vars }
            if (e.target.value) {
              newEnvVars[envName] = e.target.value
            } else {
              delete newEnvVars[envName]
            }
            onChange({ ...config, api_key_env: envName, env_vars: newEnvVars })
          }}
          placeholder={`Paste your ${config.provider || 'anthropic'} API key`}
          className="input font-mono text-xs"
        />
      </Field>
    </div>
  )
}

function Field({ label, hint, children }: { label: string; hint?: string; children: React.ReactNode }) {
  return (
    <div>
      <label className="block text-sm font-medium text-gray-300 mb-1.5">{label}</label>
      {hint && <p className="text-xs text-gray-500 mb-1.5">{hint}</p>}
      {children}
    </div>
  )
}
