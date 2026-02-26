import { useState, useEffect, useCallback } from 'react'
import { api } from '../api'
import { useToast } from '../components/Toast'
import type { User } from '../types'

export default function UsersPage() {
  const { toast } = useToast()
  const [users, setUsers] = useState<User[]>([])
  const [loading, setLoading] = useState(true)
  const [showCreate, setShowCreate] = useState(false)
  const [creating, setCreating] = useState(false)
  const [newEmail, setNewEmail] = useState('')
  const [newName, setNewName] = useState('')
  const [newRole, setNewRole] = useState<'user' | 'admin'>('user')
  const [deletingId, setDeletingId] = useState<string | null>(null)

  const loadUsers = useCallback(async () => {
    try {
      const res = await api.listUsers()
      setUsers(res.users)
    } catch (e: any) {
      toast(e.message, 'error')
    } finally {
      setLoading(false)
    }
  }, [toast])

  useEffect(() => {
    loadUsers()
  }, [loadUsers])

  const handleCreate = async (e: React.FormEvent) => {
    e.preventDefault()
    try {
      setCreating(true)
      await api.createUser({ email: newEmail, name: newName, role: newRole })
      toast(`User "${newEmail}" created`)
      setShowCreate(false)
      setNewEmail('')
      setNewName('')
      setNewRole('user')
      loadUsers()
    } catch (e: any) {
      toast(e.message, 'error')
    } finally {
      setCreating(false)
    }
  }

  const handleDelete = async (user: User) => {
    if (!confirm(`Delete user "${user.email}"? This will also delete their profile and stop their gateway.`)) {
      return
    }
    try {
      setDeletingId(user.id)
      await api.deleteUser(user.id)
      toast(`User "${user.email}" deleted`)
      loadUsers()
    } catch (e: any) {
      toast(e.message, 'error')
    } finally {
      setDeletingId(null)
    }
  }

  if (loading) {
    return (
      <div className="flex items-center justify-center h-64">
        <div className="animate-spin w-6 h-6 border-2 border-accent border-t-transparent rounded-full" />
      </div>
    )
  }

  return (
    <div>
      {/* Header */}
      <div className="flex items-center justify-between mb-6">
        <div>
          <h1 className="text-2xl font-bold text-white">Users</h1>
          <p className="text-sm text-gray-500 mt-1">{users.length} users</p>
        </div>
        <button
          onClick={() => setShowCreate(true)}
          className="px-4 py-2 text-sm font-medium rounded-lg bg-accent text-white hover:bg-accent-light transition"
        >
          + New User
        </button>
      </div>

      {/* Create user dialog */}
      {showCreate && (
        <div className="bg-surface rounded-xl border border-gray-700/50 p-5 mb-6">
          <h3 className="text-sm font-medium text-white mb-4">Create User</h3>
          <form onSubmit={handleCreate} className="space-y-3">
            <div>
              <label className="block text-xs text-gray-500 mb-1">Email</label>
              <input
                type="email"
                value={newEmail}
                onChange={(e) => setNewEmail(e.target.value)}
                placeholder="user@example.com"
                className="input text-sm"
                required
                autoFocus
              />
            </div>
            <div>
              <label className="block text-xs text-gray-500 mb-1">Display Name</label>
              <input
                value={newName}
                onChange={(e) => setNewName(e.target.value)}
                placeholder="Alice"
                className="input text-sm"
                required
              />
            </div>
            <div>
              <label className="block text-xs text-gray-500 mb-1">Role</label>
              <select
                value={newRole}
                onChange={(e) => setNewRole(e.target.value as 'user' | 'admin')}
                className="input text-sm"
              >
                <option value="user">User</option>
                <option value="admin">Admin</option>
              </select>
            </div>
            <div className="flex gap-2 pt-2">
              <button
                type="submit"
                disabled={creating}
                className="px-4 py-2 text-sm font-medium rounded-lg bg-accent text-white hover:bg-accent-light transition disabled:opacity-50"
              >
                {creating ? 'Creating...' : 'Create'}
              </button>
              <button
                type="button"
                onClick={() => setShowCreate(false)}
                className="px-4 py-2 text-sm font-medium text-gray-400 hover:text-white rounded-lg hover:bg-white/5 transition"
              >
                Cancel
              </button>
            </div>
          </form>
        </div>
      )}

      {/* User table */}
      {users.length > 0 ? (
        <div className="bg-surface rounded-xl border border-gray-700/50 overflow-hidden">
          <table className="w-full">
            <thead>
              <tr className="border-b border-gray-700/50">
                <th className="text-left px-4 py-3 text-xs font-medium text-gray-500 uppercase">Email</th>
                <th className="text-left px-4 py-3 text-xs font-medium text-gray-500 uppercase">Name</th>
                <th className="text-left px-4 py-3 text-xs font-medium text-gray-500 uppercase">Role</th>
                <th className="text-left px-4 py-3 text-xs font-medium text-gray-500 uppercase">Created</th>
                <th className="text-left px-4 py-3 text-xs font-medium text-gray-500 uppercase">Last Login</th>
                <th className="text-right px-4 py-3 text-xs font-medium text-gray-500 uppercase">Actions</th>
              </tr>
            </thead>
            <tbody>
              {users.map((u) => (
                <tr key={u.id} className="border-b border-gray-700/30 last:border-0">
                  <td className="px-4 py-3 text-sm text-white font-mono">{u.email}</td>
                  <td className="px-4 py-3 text-sm text-gray-300">{u.name}</td>
                  <td className="px-4 py-3">
                    <span
                      className={`inline-flex px-2 py-0.5 text-[10px] font-medium rounded-full ${
                        u.role === 'admin'
                          ? 'bg-amber-500/15 text-amber-400'
                          : 'bg-gray-500/15 text-gray-400'
                      }`}
                    >
                      {u.role}
                    </span>
                  </td>
                  <td className="px-4 py-3 text-xs text-gray-500">
                    {new Date(u.created_at).toLocaleDateString()}
                  </td>
                  <td className="px-4 py-3 text-xs text-gray-500">
                    {u.last_login_at ? new Date(u.last_login_at).toLocaleDateString() : 'Never'}
                  </td>
                  <td className="px-4 py-3 text-right">
                    <button
                      onClick={() => handleDelete(u)}
                      disabled={deletingId === u.id}
                      className="text-xs text-red-400 hover:text-red-300 disabled:opacity-50"
                    >
                      {deletingId === u.id ? 'Deleting...' : 'Delete'}
                    </button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      ) : (
        <div className="text-center py-16">
          <h3 className="text-lg font-medium text-gray-400 mb-2">No users yet</h3>
          <p className="text-sm text-gray-500">
            Users can self-register via email OTP if enabled, or you can create them manually.
          </p>
        </div>
      )}
    </div>
  )
}
