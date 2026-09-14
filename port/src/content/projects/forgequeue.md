---

title: FORGEqueue

slug: forgequeue

tagline: A Redis-backed distributed task queue with gRPC and concurrent workers.

description: A lightweight distributed task-processing system written in Go with a gRPC gateway, Redis-backed queues, delayed execution, and a concurrent worker pool.

status: in progress

year: "2026"

tags:

- distributed-systems

- task-queue

- concurrency

- backend

stack:

- Go

- Redis

- gRPC

- Protocol Buffers

github: https://github.com/Yashbhu/FORGEqueue

featured: true

---

  

## Overview

  

FORGEqueue is a distributed task queue built around a gRPC gateway, Redis, and a concurrent worker pool.

  

Clients submit tasks through gRPC. The gateway validates the request, assigns task metadata and a UUID, and routes the task into Redis. Workers consume tasks concurrently and dispatch them to registered handlers.

  

## Architecture

  

```text

Client

↓ gRPC

Gateway

↓

Redis

↓

Worker Pool

```

  

## What it does

  

- Accepts tasks through a gRPC API.

- Validates task requests at the gateway.

- Assigns UUIDs and per-task metadata.

- Supports immediate execution through a Redis blocking list.

- Supports delayed execution through Redis sorted sets.

- Runs tasks through a concurrent worker pool.

- Dispatches tasks by task type.

- Handles graceful shutdown with context cancellation and synchronization.

- Provides retry-oriented task metadata.

  

## Repository structure

  

```text

cmd/gateway/ gRPC gateway

internal/gateway/ gRPC server and task routing

internal/model/ task metadata

internal/redisutil Redis client utilities

internal/worker/ workers and handlers

proto/v1/ protobuf definitions

```

  

## Current state

  

The gateway, Redis queue, delayed-task representation, worker pool, and handler dispatch are implemented. Retry execution and promotion of delayed tasks into the ready queue are still being developed.