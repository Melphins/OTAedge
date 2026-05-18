'use client';

import { useEffect, useState } from 'react';
import { useRouter } from 'next/navigation';
import Link from 'next/link';
import { api, type Alert } from '@/lib/api';

export default function AlertsPage() {
  const [alerts, setAlerts] = useState<Alert[]>([]);
  const [loading, setLoading] = useState(true);
  const router = useRouter();

  useEffect(() => {
    const token = localStorage.getItem('token');
    if (!token) {
      router.push('/login');
      return;
    }

    const fetchAlerts = async () => {
      try {
        const data = await api.alerts.list(50);
        setAlerts(data);
      } catch (err) {
        console.error(err);
      } finally {
        setLoading(false);
      }
    };

    fetchAlerts();
  }, [router]);

  const handleAcknowledge = async (id: string) => {
    try {
      await api.alerts.acknowledge(id);
      setAlerts(alerts.map(a => a.id === id ? { ...a, status: 'acknowledged' } : a));
    } catch (err) {
      console.error(err);
    }
  };

  const handleClose = async (id: string) => {
    try {
      await api.alerts.close(id);
      setAlerts(alerts.map(a => a.id === id ? { ...a, status: 'closed' } : a));
    } catch (err) {
      console.error(err);
    }
  };

  const getSeverityColor = (severity: string) => {
    switch (severity.toLowerCase()) {
      case 'critical': return 'bg-red-100 text-red-800 border-red-300';
      case 'warning': return 'bg-yellow-100 text-yellow-800 border-yellow-300';
      case 'info': return 'bg-blue-100 text-blue-800 border-blue-300';
      default: return 'bg-gray-100 text-gray-800 border-gray-300';
    }
  };

  const getStatusBadge = (status: string) => {
    switch (status.toLowerCase()) {
      case 'open': return <span className="px-2 py-1 text-xs bg-green-100 text-green-800 rounded">Open</span>;
      case 'acknowledged': return <span className="px-2 py-1 text-xs bg-yellow-100 text-yellow-800 rounded">Acknowledged</span>;
      case 'silenced': return <span className="px-2 py-1 text-xs bg-gray-100 text-gray-800 rounded">Silenced</span>;
      case 'closed': return <span className="px-2 py-1 text-xs bg-gray-100 text-gray-500 rounded">Closed</span>;
      default: return <span className="px-2 py-1 text-xs bg-gray-100 text-gray-800 rounded">{status}</span>;
    }
  };

  if (loading) {
    return <div className="min-h-screen flex items-center justify-center">Loading...</div>;
  }

  const openAlerts = alerts.filter(a => a.status === 'open');
  const acknowledgedAlerts = alerts.filter(a => a.status === 'acknowledged');
  const closedAlerts = alerts.filter(a => a.status === 'closed');

  return (
    <div className="min-h-screen bg-gray-100">
      <nav className="bg-white shadow">
        <div className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8">
          <div className="flex justify-between h-16">
            <div className="flex items-center space-x-8">
              <h1 className="text-xl font-semibold text-gray-900">OTAedge</h1>
              <Link href="/dashboard" className="text-gray-600 hover:text-gray-900 px-3 py-2 text-sm font-medium">
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
              <Link href="/alerts" className="text-indigo-600 border-b-2 border-indigo-600 px-3 py-2 text-sm font-medium">
                Alerts
              </Link>
            </div>
            <div className="flex items-center">
              <button
                onClick={() => { localStorage.removeItem('token'); router.push('/login'); }}
                className="ml-4 px-4 py-2 text-sm text-gray-700 hover:text-gray-900"
              >
                Logout
              </button>
            </div>
          </div>
        </div>
      </nav>

      <main className="max-w-7xl mx-auto py-6 sm:px-6 lg:px-8">
        <div className="px-4 py-6 sm:px-0">
          <h2 className="text-2xl font-bold text-gray-900 mb-6">Alerts</h2>

          {/* Summary Cards */}
          <div className="grid grid-cols-1 md:grid-cols-4 gap-4 mb-6">
            <div className="bg-white shadow rounded-lg p-4">
              <div className="text-3xl font-bold text-red-600">{openAlerts.length}</div>
              <div className="text-sm text-gray-600">Open Alerts</div>
            </div>
            <div className="bg-white shadow rounded-lg p-4">
              <div className="text-3xl font-bold text-yellow-600">{acknowledgedAlerts.length}</div>
              <div className="text-sm text-gray-600">Acknowledged</div>
            </div>
            <div className="bg-white shadow rounded-lg p-4">
              <div className="text-3xl font-bold text-gray-600">{alerts.length}</div>
              <div className="text-sm text-gray-600">Total Alerts</div>
            </div>
            <div className="bg-white shadow rounded-lg p-4">
              <div className="text-3xl font-bold text-green-600">{closedAlerts.length}</div>
              <div className="text-sm text-gray-600">Resolved</div>
            </div>
          </div>

          {/* Open Alerts */}
          <div className="mb-8">
            <h3 className="text-lg font-medium text-gray-900 mb-4">Open Alerts</h3>
            <div className="bg-white shadow rounded-lg divide-y">
              {openAlerts.length === 0 ? (
                <div className="p-6 text-gray-500">No open alerts</div>
              ) : (
                openAlerts.map((alert) => (
                  <div key={alert.id} className="p-4">
                    <div className={`border-l-4 ${getSeverityColor(alert.severity)} p-3 rounded`}>
                      <div className="flex justify-between items-start">
                        <div className="flex-1">
                          <div className="flex items-center space-x-2 mb-1">
                            <span className="font-medium text-gray-900">{alert.title}</span>
                            {getStatusBadge(alert.status)}
                          </div>
                          <p className="text-sm text-gray-600">{alert.description}</p>
                          <div className="text-xs text-gray-500 mt-2">
                            Source: {alert.source} | {new Date(alert.created_at).toLocaleString()}
                          </div>
                        </div>
                        <div className="flex space-x-2 ml-4">
                          <button
                            onClick={() => handleAcknowledge(alert.id)}
                            className="px-3 py-1 text-sm bg-yellow-500 text-white rounded hover:bg-yellow-600"
                          >
                            Acknowledge
                          </button>
                          <button
                            onClick={() => handleClose(alert.id)}
                            className="px-3 py-1 text-sm bg-green-500 text-white rounded hover:bg-green-600"
                          >
                            Close
                          </button>
                        </div>
                      </div>
                    </div>
                  </div>
                ))
              )}
            </div>
          </div>

          {/* Acknowledged Alerts */}
          <div className="mb-8">
            <h3 className="text-lg font-medium text-gray-900 mb-4">Acknowledged Alerts</h3>
            <div className="bg-white shadow rounded-lg divide-y">
              {acknowledgedAlerts.length === 0 ? (
                <div className="p-6 text-gray-500">No acknowledged alerts</div>
              ) : (
                acknowledgedAlerts.map((alert) => (
                  <div key={alert.id} className="p-4">
                    <div className="border-l-4 border-yellow-400 p-3 rounded">
                      <div className="flex justify-between items-start">
                        <div>
                          <div className="flex items-center space-x-2 mb-1">
                            <span className="font-medium text-gray-900">{alert.title}</span>
                            {getStatusBadge(alert.status)}
                          </div>
                          <p className="text-sm text-gray-600">{alert.description}</p>
                          <div className="text-xs text-gray-500 mt-2">
                            {new Date(alert.created_at).toLocaleString()}
                          </div>
                        </div>
                        <button
                          onClick={() => handleClose(alert.id)}
                          className="px-3 py-1 text-sm bg-green-500 text-white rounded hover:bg-green-600"
                        >
                          Close
                        </button>
                      </div>
                    </div>
                  </div>
                ))
              )}
            </div>
          </div>

          {/* Closed Alerts */}
          <div>
            <h3 className="text-lg font-medium text-gray-900 mb-4">Closed Alerts</h3>
            <div className="bg-white shadow rounded-lg divide-y">
              {closedAlerts.length === 0 ? (
                <div className="p-6 text-gray-500">No closed alerts</div>
              ) : (
                closedAlerts.map((alert) => (
                  <div key={alert.id} className="p-4">
                    <div className="border-l-4 border-gray-400 p-3 rounded opacity-75">
                      <div className="flex items-center space-x-2">
                        <span className="font-medium text-gray-700">{alert.title}</span>
                        {getStatusBadge(alert.status)}
                      </div>
                      <p className="text-sm text-gray-600 mt-1">{alert.description}</p>
                      <div className="text-xs text-gray-500 mt-2">
                        {new Date(alert.created_at).toLocaleString()}
                      </div>
                    </div>
                  </div>
                ))
              )}
            </div>
          </div>
        </div>
      </main>
    </div>
  );
}