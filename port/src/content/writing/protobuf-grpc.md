---
title: Protobuf
slug: forgequeue-protobuf-grpc
published: true
date: 2026-06-20
description: Understanding gRPC and Protocol Buffers while building ForgeQueue.
tags:
  - Go
readingTime: 8 min
links:
  - ForgeQueue
---
# Understanding gRPC and Protocol Buffers While Building ForgeQueue

When I first came across Protocol Buffers and gRPC, they felt like one of those technologies everyone recommended but nobody properly explained. I knew they generated code and were supposed to be faster than REST, but while building ForgeQueue I realized they’re really about one thing: giving different services a common language to communicate with each other.

## The Core Problem

Imagine a distributed system:

`Client  |Gateway  |Worker  |Dashboard`

The Gateway wants to enqueue tasks.

The Worker wants to process tasks.

The Dashboard wants task status.

All of these are separate programs running in separate processes, and potentially on separate machines.

The challenge is simple:

> How do these programs agree on what data looks like and what operations are available?

---

## Traditional REST Approach

With REST, we manually create endpoints:

`POST /enqueuePOST /retryGET /status`

Requests are usually JSON:

`{  "task_type": "email",  "delay_seconds": 60}`

This works, but every service must manually define:

- Routes
    
- Request schemas
    
- Response schemas
    
- Validation
    
- Documentation
    

## As systems grow, maintaining these contracts becomes difficult. ![](https://yaxhmeh.vercel.app/pasted-image-20260620182027.png)

## Protocol Buffers: A Universal Contract

Protocol Buffers solve this by introducing a language-neutral contract.

Instead of describing requests using documentation or JSON examples, we define them in a `.proto` file.

`syntax = "proto3"; package queue.v1; option go_package = "forgequeue/proto/v1;queuev1"; message EnqueueTaskRequest {    string task_type = 1;    bytes payload = 2;    int32 max_retries = 3;    int64 delay_seconds = 4;} message EnqueueTaskResponse {    string task_id = 1;    string status = 2;    int64 timestamp = 3;}`

A useful mental model is:

`Go Struct      ↓Protocol Buffer Message      ↓Generated Go StructGenerated Python ClassGenerated Rust StructGenerated Java Class`

The `.proto` file becomes the source of truth.

![](https://yaxhmeh.vercel.app/pasted-image-20260620182059.png)

## Messages: The Data Shapes

A `message` is essentially a network-safe struct.

This:

`message EnqueueTaskRequest {    string task_type = 1;    bytes payload = 2;}`

is conceptually similar to:

`type EnqueueTaskRequest struct {    TaskType string    Payload  []byte}`

Messages answer one question:![](https://yaxhmeh.vercel.app/screenshot-2026-06-20-at-6.43.20-pm.png)

> What data is being exchanged?

---

## Services: The Available Actions

A service defines operations that other systems can invoke.

`service QueueService {    rpc EnqueueTask(EnqueueTaskRequest)        returns (EnqueueTaskResponse);}`

Think of this as:

`type QueueService interface {    EnqueueTask(...)}`

or, in REST terms:

`POST /enqueue`

The service answers:

> What actions are available?

Messages describe data.

Services describe behavior.

---

## Why the Versioned Package?

`package queue.v1;`

Versioning allows multiple API generations to coexist.

Today:

`queue.v1`

Tomorrow:

`queue.v2`

Old clients can continue using v1 while new clients migrate to v2.

---

## Generating Go Code

The `.proto` file itself is only a blueprint.

We compile it:

`protoc \  --go_out=. \  --go-grpc_out=. \  proto/v1/queue.proto`

This generates files such as:

`queue.pb.goqueue_grpc.pb.go`

These contain:

- Request structs
    
- Response structs
    
- Service interfaces
    
- Client stubs
    
- Server stubs
    

This is where types such as:

`queuev1.EnqueueTaskRequest`

actually come from.

---

## Implementing Business Logic

The generated code only defines contracts.

It does not define behavior.

We implement behavior ourselves:

`func (s *server) EnqueueTask(    ctx context.Context,    req *queuev1.EnqueueTaskRequest,) (*queuev1.EnqueueTaskResponse, error) {     taskID := uuid.New().String()     err := s.trouter.RouteTask(        ctx,        taskID,        req.TaskType,        req.Payload,        req.MaxRetries,        req.DelaySeconds,    )     if err != nil {        return nil, err    }     return &queuev1.EnqueueTaskResponse{        TaskId: taskID,        Status: "QUEUED",    }, nil}`

The generated contract defines what the API should look like.

Our implementation defines what the API actually does.

---

## Why gRPC Uses HTTP/2

gRPC uses HTTP/2 underneath.

Traditional HTTP/1.1 suffers from head-of-line blocking and often requires multiple TCP connections to achieve high throughput.

HTTP/2 introduces:

- Multiplexing
    
- Header compression (HPACK)
    
- Persistent connections
    
- Bidirectional streaming
    

Instead of opening many TCP connections:

`Client ├── Connection 1 ├── Connection 2 ├── Connection 3`

HTTP/2 allows:

`Client     |Single TCP Connection     |Hundreds of Streams`

This significantly reduces overhead.

---

## Why Protocol Buffers Are Fast

JSON is text:

`{  "task_type": "email"}`

Before a computer can use it, it must parse the text.

Protocol Buffers serialize data into compact binary.

Instead of sending field names repeatedly:

`{  "task_type": "email"}`

Protobuf sends field identifiers and values.

This reduces:

- Network payload size
    
- CPU parsing cost
    
- Memory allocations
    

---

## How Protobuf Identifies Fields

![](https://yaxhmeh.vercel.app/pasted-image-20260620182210.png) Each field has a unique tag number.

`string task_type = 1;bytes payload = 2;`

![](https://yaxhmeh.vercel.app/pasted-image-20260620182237.png) The numbers are not indexes.

They are permanent field identifiers.

Internally, protobuf encodes data as Tag-Length-Value structures.

The parser reads:

`Tag↓Which field is this? Length↓How many bytes belong to it? Value↓Actual data`

This allows fields to appear in any order and enables backward compatibility.

---

## Mental Model

The simplest mental model I found is:

`Message=What data exists? Service=What actions exist? Generated Code=Strongly typed implementation scaffolding gRPC=Network function calls Protocol Buffers=Compact binary representation`

Or even more simply:

`.proto↓Blueprint protoc↓Generates code Implementation↓Adds business logic gRPC↓Exposes it over the network`