'use client';

import { useEffect, useState } from 'react';
import { useRouter } from 'next/navigation';
import Link from 'next/link';
import type { Device, Model, Deployment, Alert } from '@/lib/api';

const API_URL = process.env.NEXT_PUBLIC_API_URL || 'http://localhost:3000';

interface Stats {
  totalDevices: number;
  onlineDevices: number;
  offlineDevices: number;
  activeDeployments: number;
  pendingAlerts: number;
  totalModels: number;
}

export default function DashboardPage() {
  const [devices, setDevices] = useState<Device[]>([]);
  const [models, setModels] = useState<Model[]>([]);
  const [deployments, setDeployments] = useState<Deployment[]>([]);
  const [alerts, setAlerts] = useState<Alert[]>([]);
  const [stats, setStats] = useState<Stats>({
    totalDevices: 0,
    onlineDevices: 0,
    offlineDevices: 0,
    activeDeployments: 0,
    pendingAlerts: 0,
    totalModels: 0,
  });
  const [loading, setLoading] = useState(true);
  const router = useRouter();

  useEffect(() => {
    const token = localStorage.getItem('token');
    if (!token) {
      router.push('/login');
      return;
    }

    const fetchData = async () => {
      try {
        const [devRes, modRes, depRes, altRes] = await Promise.all([
          fetch(`${API_URL}/api/devices`, {
            headers: { Authorization: `Bearer ${token}` },
          }),
          fetch(`${API_URL}/api/models`, {
            headers: { Authorization: `Bearer ${token}` },
          }),
          fetch(`${API_URL}/api/deployments`, {
            headers: { Authorization: `Bearer ${token}` },
          }),
          fetch(`${API_URL}/api/alerts`, {
            headers: { Authorization: `Bearer ${token}` },
          }).catch(() => ({ ok: false, status: 0 } as Response)),
        ]);

        const devData = devRes.ok ? await devRes.json() : [];
        const modData = modRes.ok ? await modRes.json() : [];
        const depData = depRes.ok ? await depRes.json() : [];
        const altResOk = altRes && 'ok' in altRes && altRes.ok;
        const altData = altResOk ? await altRes.json() : [];

        const devicesArray = Array.isArray(devData) ? devData : devData.devices || [];
        const modelsArray = Array.isArray(modData) ? modData : modData.models || [];
        const deploymentsArray = Array.isArray(depData) ? depData : depData.deployments || [];
        const alertsArray = Array.isArray(altData) ? altData : altData.alerts || [];

        setDevices(devicesArray);
        setModels(modelsArray);
        setDeployments(deploymentsArray);
        setAlerts(alertsArray);

        // Calculate stats
        const onlineCount = devicesArray.filter((d: Device) => d.status === 'online').length;
        setStats({
          totalDevices: devicesArray.length,
          onlineDevices: onlineCount,
          offlineDevices: devicesArray.length - onlineCount,
          activeDeployments: deploymentsArray.filter((d: Deployment) => d.status === 'deploying').length,
          pendingAlerts: alertsArray.filter((a: Alert) => a.status !== 'closed').length,
          totalModels: modelsArray.length,
        });
      } catch (err) {
        console.error(err);
      } finally {
        setLoading(false);
      }
    };

    fetchData();
  }, [router]);

  const handleLogout = () => {
    localStorage.removeItem('token');
    router.push('/login');
  };

  const getStatusColor = (status: string) => {
    switch (status) {
      case 'online':
        return 'bg-green-100 text-green-800';
      case 'offline':
        return 'bg-red-100 text-red-800';
      case 'deploying':
        return 'bg-blue-100 text-blue-800';
      case 'completed':
        return 'bg-green-100 text-green-800';
      case 'failed':
        return 'bg-red-100 text-red-800';
      case 'open':
        return 'bg-yellow-100 text-yellow-800';
      case 'acknowledged':
        return 'bg-blue-100 text-blue-800';
      default:
        return 'bg-gray-100 text-gray-800';
    }
  };

  if (loading) {
    return (
      <div className="min-h-screen flex items-center justify-center bg-gray-100">
        <div className="text-center">
          <div className="animate-spin rounded-full h-12 w-12 border-b-2 border-indigo-600 mx-auto"></div>
          <p className="mt-4 text-gray-600">Loading dashboard...</p>
        </div>
      </div>
    );
  }

  return (
    <div className="min-h-screen bg-gray-50">
      {/* Navigation */}
      <nav className="bg-white shadow-sm border-b border-gray-200">
        <div className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8">
          <div className="flex justify-between h-16">
            <div className="flex items-center space-x-8">
              <div className="flex items-center">
                <svg className="h-8 w-8 text-indigo-600" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                  <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M5 12h14M5 12a2 2 0 01-2-2V6a2 2 0 012-2h14a2 2 0 012 2v4a2 2 0 01-2 2M5 12a2 2 0 00-2 2v4a2 2 0 002 2h14a2 2 0 002-2v-4a2 2 0 00-2-2m-2-4h.01M17 16h.01" />
                </svg>
                <span className="ml-2 text-xl font-bold text-gray-900">OTAedge</span>
              </div>
              <div className="flex items-center space-x-4">
                <Link href="/dashboard" className="text-indigo-600 border-b-2 border-indigo-600 px-3 py-2 text-sm font-medium">
                  Dashboard
                </Link>
                <Link href="/devices" className="text-gray-600 hover:text-gray-900 px-3 py-2 text-sm font-medium">
                  Devices
                </Link>
                <Link href="/models" className="text-gray-600 hover:text-gray-900 px-3 py-2 text-sm font-medium">
                  Models
                </Link>
                <Link href="/deployments" className="text-gray-600 hover:text-gray-900 px-3 py-2 text-sm font-medium">
                  Deployments
                </Link>
                <Link href="/alerts" className="text-gray-600 hover:text-gray-900 px-3 py-2 text-sm font-medium relative">
                  Alerts
                  {stats.pendingAlerts > 0 && (
                    <span className="absolute -top-1 -right-2 bg-red-500 text-white text-xs rounded-full h-5 w-5 flex items-center justify-center">
                      {stats.pendingAlerts}
                    </span>
                  )}
                </Link>
              </div>
            </div>
            <div className="flex items-center">
              <button
                onClick={handleLogout}
                className="ml-4 px-4 py-2 text-sm font-medium text-gray-700 hover:text-gray-900 hover:bg-gray-100 rounded-md transition-colors"
              >
                Logout
              </button>
            </div>
          </div>
        </div>
      </nav>

      {/* Main Content */}
      <main className="max-w-7xl mx-auto py-6 sm:px-6 lg:px-8">
        <div className="px-4 py-6 sm:px-0">
          {/* Stats Cards */}
          <div className="grid grid-cols-1 gap-6 sm:grid-cols-2 lg:grid-cols-4 mb-8">
            {/* Total Devices */}
            <div className="bg-white overflow-hidden shadow-sm rounded-lg border border-gray-200">
              <div className="p-6">
                <div className="flex items-center">
                  <div className="flex-shrink-0 bg-indigo-100 rounded-md p-3">
                    <svg className="h-6 w-6 text-indigo-600" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                      <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M5 12h14M5 12a2 2 0 01-2-2V6a2 2 0 012-2h14a2 2 0 012 2v4a2 2 0 01-2 2M5 12a2 2 0 00-2 2v4a2 2 0 002 2h14a2 2 0 002-2v-4a2 2 0 00-2-2m-2-4h.01M17 16h.01" />
                    </svg>
                  </div>
                  <div className="ml-4">
                    <p className="text-sm font-medium text-gray-500">Total Devices</p>
                    <p className="text-2xl font-semibold text-gray-900">{stats.totalDevices}</p>
                  </div>
                </div>
              </div>
            </div>

            {/* Online Devices */}
            <div className="bg-white overflow-hidden shadow-sm rounded-lg border border-gray-200">
              <div className="p-6">
                <div className="flex items-center">
                  <div className="flex-shrink-0 bg-green-100 rounded-md p-3">
                    <svg className="h-6 w-6 text-green-600" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                      <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M9 12l2 2 4-4m6 2a9 9 0 11-18 0 9 9 0 0118 0z" />
                    </svg>
                  </div>
                  <div className="ml-4">
                    <p className="text-sm font-medium text-gray-500">Online</p>
                    <p className="text-2xl font-semibold text-green-600">{stats.onlineDevices}</p>
                  </div>
                </div>
              </div>
            </div>

            {/* Active Deployments */}
            <div className="bg-white overflow-hidden shadow-sm rounded-lg border border-gray-200">
              <div className="p-6">
                <div className="flex items-center">
                  <div className="flex-shrink-0 bg-blue-100 rounded-md p-3">
                    <svg className="h-6 w-6 text-blue-600" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                      <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M13 10V3L4 14h7v7l9-11h-7z" />
                    </svg>
                  </div>
                  <div className="ml-4">
                    <p className="text-sm font-medium text-gray-500">Active Deployments</p>
                    <p className="text-2xl font-semibold text-blue-600">{stats.activeDeployments}</p>
                  </div>
                </div>
              </div>
            </div>

            {/* Pending Alerts */}
            <div className="bg-white overflow-hidden shadow-sm rounded-lg border border-gray-200">
              <div className="p-6">
                <div className="flex items-center">
                  <div className="flex-shrink-0 bg-yellow-100 rounded-md p-3">
                    <svg className="h-6 w-6 text-yellow-600" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                      <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M12 9v2m0 4h.01m-6.938 4h13.856c1.54 0 2.502-1.667 1.732-3L13.732 4c-.77-1.333-2.694-1.333-3.464 0L3.34 16c-.77 1.333.192 3 1.732 3z" />
                    </svg>
                  </div>
                  <div className="ml-4">
                    <p className="text-sm font-medium text-gray-500">Pending Alerts</p>
                    <p className="text-2xl font-semibold text-yellow-600">{stats.pendingAlerts}</p>
                  </div>
                </div>
              </div>
            </div>
          </div>

          <div className="grid grid-cols-1 gap-6 lg:grid-cols-3">
            {/* Device Status Overview */}
            <div className="lg:col-span-2 bg-white shadow-sm rounded-lg border border-gray-200">
              <div className="p-6">
                <div className="flex items-center justify-between mb-4">
                  <h2 className="text-lg font-semibold text-gray-900">Device Status Overview</h2>
                  <Link href="/devices" className="text-sm text-indigo-600 hover:text-indigo-900 font-medium">
                    View all →
                  </Link>
                </div>
                <div className="space-y-3">
                  {devices.slice(0, 5).map((device) => (
                    <div key={device.id} className="flex items-center justify-between p-3 bg-gray-50 rounded-lg hover:bg-gray-100 transition-colors">
                      <div className="flex items-center">
                        <div className={`w-2 h-2 rounded-full ${device.status === 'online' ? 'bg-green-500' : 'bg-red-500'}`}></div>
                        <div className="ml-3">
                          <p className="text-sm font-medium text-gray-900">{device.name || device.device_id}</p>
                          <p className="text-xs text-gray-500">{device.device_type || 'unknown'}</p>
                        </div>
                      </div>
                      <span
                        className={`px-2 py-1 text-xs font-medium rounded-full ${getStatusColor(device.status)}`}
                      >
                        {device.status}
                      </span>
                    </div>
                  ))}
                  {devices.length === 0 && (
                    <p className="text-center text-gray-500 py-8">No devices registered</p>
                  )}
                </div>
              </div>
            </div>

            {/* Models Overview */}
            <div className="bg-white shadow-sm rounded-lg border border-gray-200">
              <div className="p-6">
                <div className="flex items-center justify-between mb-4">
                  <h2 className="text-lg font-semibold text-gray-900">Models</h2>
                  <Link href="/models/upload" className="text-sm text-indigo-600 hover:text-indigo-900 font-medium">
                    Upload →
                  </Link>
                </div>
                <div className="space-y-3">
                  {models.slice(0, 5).map((model) => (
                    <div key={model.id} className="p-3 bg-gray-50 rounded-lg">
                      <div className="flex items-center justify-between">
                        <div>
                          <p className="text-sm font-medium text-gray-900">{model.name}</p>
                          <p className="text-xs text-gray-500">v{model.version} • {model.model_format || 'unknown'}</p>
                        </div>
                        {model.is_active && (
                          <span className="px-2 py-1 text-xs font-medium rounded-full bg-green-100 text-green-800">
                            Active
                          </span>
                        )}
                      </div>
                    </div>
                  ))}
                  {models.length === 0 && (
                    <p className="text-center text-gray-500 py-8">No models uploaded</p>
                  )}
                </div>
              </div>
            </div>
          </div>

          {/* Active Deployments */}
          {stats.activeDeployments > 0 && (
            <div className="mt-6 bg-white shadow-sm rounded-lg border border-gray-200">
              <div className="p-6">
                <div className="flex items-center justify-between mb-4">
                  <h2 className="text-lg font-semibold text-gray-900">Active Deployments</h2>
                  <Link href="/deployments" className="text-sm text-indigo-600 hover:text-indigo-900 font-medium">
                    View all →
                  </Link>
                </div>
                <div className="space-y-4">
                  {deployments
                    .filter((d) => d.status === 'deploying')
                    .slice(0, 3)
                    .map((deployment) => {
                      const progress = (deployment.devices_target ?? 0) > 0
                        ? Math.round(((deployment.devices_deployed ?? 0) / (deployment.devices_target ?? 1)) * 100)
                        : 0;
                      return (
                        <div key={deployment.id} className="p-4 bg-gray-50 rounded-lg">
                          <div className="flex items-center justify-between mb-2">
                            <div>
                              <p className="text-sm font-medium text-gray-900">Deployment in Progress</p>
                              <p className="text-xs text-gray-500">
                                {deployment.devices_deployed}/{deployment.devices_target} devices
                              </p>
                            </div>
                            <span className="px-2 py-1 text-xs font-medium rounded-full bg-blue-100 text-blue-800">
                              {deployment.rollout_strategy}
                            </span>
                          </div>
                          <div className="w-full bg-gray-200 rounded-full h-2">
                            <div
                              className="bg-blue-600 h-2 rounded-full transition-all duration-300"
                              style={{ width: `${progress}%` }}
                            ></div>
                          </div>
                          <p className="text-xs text-gray-500 mt-2 text-right">{progress}% complete</p>
                        </div>
                      );
                    })}
                </div>
              </div>
            </div>
          )}

          {/* Recent Alerts */}
          {stats.pendingAlerts > 0 && (
            <div className="mt-6 bg-white shadow-sm rounded-lg border border-gray-200">
              <div className="p-6">
                <div className="flex items-center justify-between mb-4">
                  <h2 className="text-lg font-semibold text-gray-900">Recent Alerts</h2>
                  <Link href="/alerts" className="text-sm text-indigo-600 hover:text-indigo-900 font-medium">
                    View all →
                  </Link>
                </div>
                <div className="space-y-3">
                  {alerts.slice(0, 3).map((alert) => (
                    <div key={alert.id} className="flex items-center justify-between p-3 bg-gray-50 rounded-lg">
                      <div className="flex items-center">
                        <svg className="h-5 w-5 text-yellow-600" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                          <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M12 9v2m0 4h.01m-6.938 4h13.856c1.54 0 2.502-1.667 1.732-3L13.732 4c-.77-1.333-2.694-1.333-3.464 0L3.34 16c-.77 1.333.192 3 1.732 3z" />
                        </svg>
                        <div className="ml-3">
                          <p className="text-sm font-medium text-gray-900">{alert.title || 'Alert'}</p>
                          <p className="text-xs text-gray-500">{new Date(alert.created_at).toLocaleString()}</p>
                        </div>
                      </div>
                      <span className={`px-2 py-1 text-xs font-medium rounded-full ${getStatusColor(alert.status)}`}>
                        {alert.status}
                      </span>
                    </div>
                  ))}
                </div>
              </div>
            </div>
          )}

          {/* Quick Actions */}
          <div className="mt-6 grid grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-4">
            <Link
              href="/devices"
              className="flex items-center p-4 bg-white shadow-sm rounded-lg border border-gray-200 hover:border-indigo-300 hover:shadow-md transition-all"
            >
              <svg className="h-6 w-6 text-indigo-600" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M12 6v6m0 0v6m0-6h6m-6 0H6" />
              </svg>
              <span className="ml-3 text-sm font-medium text-gray-900">Add Device</span>
            </Link>
            <Link
              href="/models/upload"
              className="flex items-center p-4 bg-white shadow-sm rounded-lg border border-gray-200 hover:border-indigo-300 hover:shadow-md transition-all"
            >
              <svg className="h-6 w-6 text-indigo-600" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M7 16a4 4 0 01-.88-7.903A5 5 0 1115.9 6L16 6a5 5 0 011 9.9M15 13l-3-3m0 0l-3 3m3-3v12" />
              </svg>
              <span className="ml-3 text-sm font-medium text-gray-900">Upload Model</span>
            </Link>
            <Link
              href="/deployments"
              className="flex items-center p-4 bg-white shadow-sm rounded-lg border border-gray-200 hover:border-indigo-300 hover:shadow-md transition-all"
            >
              <svg className="h-6 w-6 text-indigo-600" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M13 10V3L4 14h7v7l9-11h-7z" />
              </svg>
              <span className="ml-3 text-sm font-medium text-gray-900">New Deployment</span>
            </Link>
            <Link
              href="/alerts"
              className="flex items-center p-4 bg-white shadow-sm rounded-lg border border-gray-200 hover:border-indigo-300 hover:shadow-md transition-all"
            >
              <svg className="h-6 w-6 text-indigo-600" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M15 17h5l-1.405-1.405A2.032 2.032 0 0118 14.158V11a6.002 6.002 0 00-4-5.659V5a2 2 0 10-4 0v.341C7.67 6.165 6 8.388 6 11v3.159c0 .538-.214 1.055-.595 1.436L4 17h5m6 0v1a3 3 0 11-6 0v-1m6 0H9" />
              </svg>
              <span className="ml-3 text-sm font-medium text-gray-900">View Alerts</span>
            </Link>
          </div>
        </div>
      </main>
    </div>
  );
}