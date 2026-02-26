import { useState, useEffect, useRef } from 'react'
import { useAuth } from '../contexts/AuthContext'
import { myApi, getLogStreamUrl } from '../api'
import { useToast } from '../components/Toast'
import type { ProfileConfig, ProcessStatus } from '../types'
import LlmProviderTab from '../components/tabs/LlmProviderTab'
import SearchApiTab from '../components/tabs/SearchApiTab'
import PptConfigTab from '../components/tabs/PptConfigTab'
import TelegramTab from '../components/tabs/TelegramTab'
import WhatsAppTab from '../components/tabs/WhatsAppTab'
import FeishuTab from '../components/tabs/FeishuTab'
import GatewayTab from '../components/tabs/GatewayTab'

type Tab = 'llm' | 'search' | 'ppt' | 'telegram' | 'whatsapp' | 'feishu' | 'gateway'

const TABS: { key: Tab; label: string }[] = [
  { key: 'llm', label: 'LLM Provider' },
  { key: 'search', label: 'Search APIs' },
  { key: 'ppt', label: 'PPT' },
  { key: 'telegram', label: 'Telegram' },
  { key: 'whatsapp', label: 'WhatsApp' },
  { key: 'feishu', label: 'Feishu' },
  { key: 'gateway', label: 'Gateway' },
]

export default function MyProfile() {
  const { user } = useAuth()
  const { toast } = useToast()
  const [config, setConfig] = useState<ProfileConfig | null>(null)
  const [status, setStatus] = useState<ProcessStatus | null>(null)
  const [loading, setLoading] = useState(true)
  const [saving, setSaving] = useState(false)
  const [actionLoading, setActionLoading] = useState(false)
  const [activeTab, setActiveTab] = useState<Tab>('llm')
  const [logs, setLogs] = useState<string[]>([])
  const [showLogs, setShowLogs] = useState(false)
  const logRef = useRef<HTMLDivElement>(null)
  const eventSourceRef = useRef<EventSource | null>(null)

  // Load profile
  useEffect(() => {
    loadProfile()
  }, [])

  // Auto-scroll logs
  useEffect(() => {
    if (logRef.current) {
      logRef.current.scrollTop = logRef.current.scrollHeight
    }
  }, [logs])

  // Cleanup event source on unmount
  useEffect(() => {
    return () => {
      eventSourceRef.current?.close()
    }
  }, [])

  const loadProfile = async () => {
    try {
      setLoading(true)
      const profile = await myApi.getProfile()
      setConfig(profile.config)
      setStatus(profile.status)
    } catch (e: any) {
      toast(e.message, 'error')
    } finally {
      setLoading(false)
    }
  }

  const handleSave = async () => {
    if (!config) return
    try {
      setSaving(true)
      const profile = await myApi.updateProfile({ config })
      setConfig(profile.config)
      setStatus(profile.status)
      toast('Configuration saved')
    } catch (e: any) {
      toast(e.message, 'error')
    } finally {
      setSaving(false)
    }
  }

  const handleStart = async () => {
    try {
      setActionLoading(true)
      await myApi.startGateway()
      toast('Gateway started')
      await loadProfile()
    } catch (e: any) {
      toast(e.message, 'error')
    } finally {
      setActionLoading(false)
    }
  }

  const handleStop = async () => {
    try {
      setActionLoading(true)
      await myApi.stopGateway()
      toast('Gateway stopped')
      await loadProfile()
    } catch (e: any) {
      toast(e.message, 'error')
    } finally {
      setActionLoading(false)
    }
  }

  const handleRestart = async () => {
    try {
      setActionLoading(true)
      await myApi.restartGateway()
      toast('Gateway restarted')
      await loadProfile()
    } catch (e: any) {
      toast(e.message, 'error')
    } finally {
      setActionLoading(false)
    }
  }

  const toggleLogs = () => {
    if (showLogs) {
      eventSourceRef.current?.close()
      eventSourceRef.current = null
      setShowLogs(false)
    } else {
      setShowLogs(true)
      setLogs([])
      const url = getLogStreamUrl()
      const es = new EventSource(url)
      es.onmessage = (event) => {
        setLogs((prev) => [...prev.slice(-500), event.data])
      }
      es.onerror = () => {
        es.close()
      }
      eventSourceRef.current = es
    }
  }

  if (loading) {
    return (
      <div className="flex items-center justify-center h-64">
        <div className="animate-spin w-6 h-6 border-2 border-accent border-t-transparent rounded-full" />
      </div>
    )
  }

  if (!config) {
    return (
      <div className="bg-red-500/10 border border-red-500/30 rounded-lg p-4 text-red-400 text-sm">
        Failed to load your profile. Please try refreshing the page.
      </div>
    )
  }

  const isRunning = status?.running === true

  return (
    <div>
      {/* Header */}
      <div className="flex items-center justify-between mb-6">
        <div>
          <h1 className="text-2xl font-bold text-white">My Profile</h1>
          <p className="text-sm text-gray-500 mt-1">
            {user?.email}
            <span className={`ml-3 inline-flex items-center gap-1.5 text-xs ${isRunning ? 'text-green-400' : 'text-gray-500'}`}>
              <span className={`w-1.5 h-1.5 rounded-full ${isRunning ? 'bg-green-400' : 'bg-gray-600'}`} />
              {isRunning ? 'Running' : 'Stopped'}
            </span>
          </p>
        </div>
        <div className="flex gap-2">
          {isRunning ? (
            <>
              <button
                onClick={handleStop}
                disabled={actionLoading}
                className="px-4 py-2 text-sm font-medium rounded-lg bg-red-500/10 text-red-400 hover:bg-red-500/20 border border-red-500/20 transition disabled:opacity-50"
              >
                Stop
              </button>
              <button
                onClick={handleRestart}
                disabled={actionLoading}
                className="px-4 py-2 text-sm font-medium rounded-lg bg-amber-500/10 text-amber-400 hover:bg-amber-500/20 border border-amber-500/20 transition disabled:opacity-50"
              >
                Restart
              </button>
            </>
          ) : (
            <button
              onClick={handleStart}
              disabled={actionLoading}
              className="px-4 py-2 text-sm font-medium rounded-lg bg-green-500/10 text-green-400 hover:bg-green-500/20 border border-green-500/20 transition disabled:opacity-50"
            >
              Start Gateway
            </button>
          )}
          <button
            onClick={toggleLogs}
            className={`px-4 py-2 text-sm font-medium rounded-lg border transition ${
              showLogs
                ? 'bg-accent/15 text-accent border-accent/30'
                : 'bg-white/5 text-gray-400 border-gray-700/50 hover:bg-white/10'
            }`}
          >
            Logs
          </button>
        </div>
      </div>

      {/* Log viewer */}
      {showLogs && (
        <div className="mb-6 bg-surface-dark rounded-lg border border-gray-700/50 overflow-hidden">
          <div className="flex items-center justify-between px-4 py-2 border-b border-gray-700/50">
            <span className="text-xs text-gray-500 font-medium">Live Logs</span>
            <button
              onClick={() => setLogs([])}
              className="text-[10px] text-gray-600 hover:text-gray-400"
            >
              Clear
            </button>
          </div>
          <div
            ref={logRef}
            className="h-48 overflow-y-auto p-3 font-mono text-[11px] text-gray-400 leading-relaxed"
          >
            {logs.length === 0 ? (
              <span className="text-gray-600">Waiting for logs...</span>
            ) : (
              logs.map((line, i) => <div key={i}>{line}</div>)
            )}
          </div>
        </div>
      )}

      {/* Config tabs */}
      <div className="bg-surface rounded-xl border border-gray-700/50 overflow-hidden">
        <div className="flex border-b border-gray-700/50 overflow-x-auto">
          {TABS.map((tab) => (
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

        <div className="p-5">
          {activeTab === 'llm' && <LlmProviderTab config={config} onChange={setConfig} />}
          {activeTab === 'search' && <SearchApiTab config={config} onChange={setConfig} />}
          {activeTab === 'ppt' && <PptConfigTab config={config} onChange={setConfig} />}
          {activeTab === 'telegram' && <TelegramTab config={config} onChange={setConfig} />}
          {activeTab === 'whatsapp' && <WhatsAppTab config={config} onChange={setConfig} />}
          {activeTab === 'feishu' && <FeishuTab config={config} onChange={setConfig} />}
          {activeTab === 'gateway' && <GatewayTab config={config} onChange={setConfig} />}
        </div>

        <div className="px-5 py-4 border-t border-gray-700/50 flex justify-end">
          <button
            onClick={handleSave}
            disabled={saving}
            className="px-6 py-2 text-sm font-medium rounded-lg bg-accent text-white hover:bg-accent-light transition disabled:opacity-50"
          >
            {saving ? 'Saving...' : 'Save Configuration'}
          </button>
        </div>
      </div>
    </div>
  )
}
