import type { ProfileConfig } from '../../types'

const SEARCH_KEYS = [
  { key: 'PERPLEXITY_API_KEY', label: 'Perplexity', placeholder: 'pplx-...' },
  { key: 'BRAVE_SEARCH_API_KEY', label: 'Brave Search', placeholder: 'BSA...' },
  { key: 'YOU_API_KEY', label: 'You.com', placeholder: '' },
]

interface Props {
  config: ProfileConfig
  onChange: (config: ProfileConfig) => void
}

export default function SearchApiTab({ config, onChange }: Props) {
  const updateEnv = (key: string, value: string) => {
    const newEnvVars = { ...config.env_vars }
    if (value) {
      newEnvVars[key] = value
    } else {
      delete newEnvVars[key]
    }
    onChange({ ...config, env_vars: newEnvVars })
  }

  return (
    <div className="space-y-4">
      <p className="text-xs text-gray-500">
        Configure API keys for web search providers. The agent will use whichever is available
        (DuckDuckGo is used by default with no API key needed).
      </p>

      {SEARCH_KEYS.map(({ key, label, placeholder }) => (
        <div key={key}>
          <label className="block text-sm font-medium text-gray-300 mb-1.5">{label}</label>
          <input
            type="password"
            value={config.env_vars[key] || ''}
            onChange={(e) => updateEnv(key, e.target.value)}
            placeholder={placeholder || 'API key'}
            className="input font-mono text-xs"
          />
          <p className="text-[10px] text-gray-600 mt-1">{key}</p>
        </div>
      ))}
    </div>
  )
}
