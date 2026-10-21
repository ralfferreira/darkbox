# Nmap adapter

## Role

Nmap adds network-level observations to an assessment: which of the allowed
ports are open and what service Nmap names them. It does not replace the web
inventory. An open port is an observation on a host asset; an HTTP endpoint
still requires an HTTP observation.

## Invocation

The worker resolves the binary from `PATH`. If it is missing, the capability
`network.nmap` is `unavailable` and the run is blocked with that reason.

The command is assembled from structured arguments, never from a shell:

```text
nmap -sT -Pn -n -T3 --max-retries 1 --host-timeout 25s \
     -p <policy ports> -oX - <scoped hosts>
```

- `-sT` is a TCP connect scan. No raw sockets or elevated privileges.
- `-Pn` avoids a host-discovery phase that could probe addresses outside the
  requested host.
- Each host is authorized separately. Each port is authorized separately.
- Only ports in `allowed_ports` are passed. A requested port outside the list
  is dropped, not scanned.
- At most 20 hosts and 200 ports per run.
- The process runs in its own process group. Timeout or cancellation kills the
  group.

## Output

The XML is parsed with a bounded reader. For each host:

- `network.address` with address, hostname and up/down status;
- `network.open_port` with port, protocol, service, product and version when
  Nmap reports them;
- the XML itself is stored as sanitized `tool_output` evidence.

Commands and the scanner version are recorded in the observation. A version
string from Nmap is not a vulnerability finding.

## Scope limits

- Hostnames are resolved once before the scan and checked for private or
  link-local addresses when `allow_private_network` is false.
- The host must be included, and must not be excluded. Exclusions win.
- The run's pinned policy and the current policy must both allow the host and
  port. Revoking a port after a run starts stops further hosts on that port.
- Nmap itself does not follow HTTP redirects. Redirect policy is handled by the
  HTTP and browser paths.

## Why no `-sV` yet

Service detection sends additional probes and can be slow or noisy. The first
release keeps the scan to port state and whatever service name the default
probe already reports. Add `-sV` only with its own procedure limits and tests.
