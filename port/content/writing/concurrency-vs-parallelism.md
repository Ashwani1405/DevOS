---
title: Concurrency vs Parallelism
slug: forgequeue-concurrency-vs-parallelism
published: true
date: 2026-08-30
description: The distinction between concurrency and parallelism, applied to the ForgeQueue worker pool.
tags:
  - concurrency
readingTime: 7 min
links:
  - ForgeQueue
---
## Tracing concurrency, parallelism, and worker scheduling while building ForgeQueue.

While working on the worker pool for ForgeQueue, I got stuck on something that I thought I already understood.

What does concurrency actually mean?

I knew the basic idea. I can have multiple workers processing different tasks.

What I wasn’t clear about was what was actually happening underneath.

The code that started this whole rabbit hole was pretty small:

![415](https://miro.medium.com/v2/1*wTdEAjU5XNAzgmwKrvsR0w.png)

I knew this was creating multiple workers, but then I started thinking about what “multiple workers” actually meant.

If I have:

> _→Worker 1  
> →Worker 2  
> →Worker 3_

and multiple CPU cores, it’s easy to picture them running at the same time.

## But what happens if I only have one CPU core?

A single core obviously can’t execute instructions from Worker 1, Worker 2, and Worker 3 at the exact same instant. It has to execute some work, move to other runnable work, and keep doing that as needed.

So we can still have multiple workers making progress even though they aren’t literally executing at the same time.

That was the first distinction that actually started making sense to me.

> **_Concurrency_** _is about multiple independent pieces of work being in progress._
> 
> **_Parallelism_** _is about multiple pieces of work actually executing at the same time._

So concurrency doesn’t require multiple CPU cores.

## But then I had another question.

If parallelism already lets us run multiple things at the same time, why do I care about concurrency?

Looking at what the workers in ForgeQueue actually do made that clearer.

Inside the worker loop, ForgeQueue waits for work from Redis:

![](https://miro.medium.com/v2/1*x0V9Eoa_xT5Cv4evCsXeqg.png)

The interesting part is this:

![](https://miro.medium.com/v2/1*GPyXCnGyP1cPW3seycu4eA.png)

`BRPop` can wait for a task.

That means a worker isn’t necessarily doing CPU computation for its entire lifetime.

Imagine three workers:

> _Worker 1 → waiting for Redis  
> Worker 2 → processing a task  
> Worker 3 → waiting for Redis_

- Worker 1 isn’t doing useful CPU computation while it’s waiting.
- Worker 3 isn’t either.
- Worker 2 can still make progress.

That’s the part I was missing.

A worker doesn’t have to be continuously using the CPU for the worker to be useful. It can be waiting for something, while other work continues, and backend systems do this constantly

### An HTTP request is a simple example:

> _send request → wait for response → process response_

The waiting isn’t the same thing as CPU computation.

The same idea applies to:

- HTTP requests
- database queries
- Redis operations
- network reads
- disk I/O
- requests to another service

A task can spend part of its lifetime computing and another part waiting for something outside the CPU.

## That also made the term **I/O** make more sense to me.

I had heard “I/O” enough times to know it meant input/output, but I hadn’t really connected it to what my workers were doing.

**For example,** when a worker communicates with Redis, the worker sends a request and waits for Redis to respond.

The same thing happens with a database:

> _Worker → send query → wait → database responds → continue processing_

There is a period where the worker can’t continue that particular piece of work because it is waiting for something else.

Once the response arrives, the work can continue and so asingle task can therefore move between computation and waiting multiple times:

![](https://miro.medium.com/v2/0*3hkCCTxYKOcGvuyo)

So now, when I think about a worker, I don’t picture it as something that’s permanently “running”.,but now it’s more like something that can move between different states depending on what it is currently waiting for.

## Data transfer made this even clearer

Suppose a worker is receiving a large amount of data over a network.

The worker isn’t spending the entire transfer doing CPU work.

There is some CPU work involved in processing the data that has already arrived, but there can also be periods where the system is simply waiting for more data.

Conceptually:

![](https://miro.medium.com/v2/1*sPAUTuOVBMZFoTikm3USEQ.png)

During those waiting periods, other runnable work can use the available execution resources.

So even though we might describe the task as **_“transferring data”,_** the task itself is moving through different phases well … sometimes it’s doing computation and sometimes it’s just waiting for I/O.

That is where having multiple pieces of work in progress becomes useful for us.

## But what if the task is CPU-heavy?

This is where parallelism matters.

Suppose Worker 1, Worker 2, Worker 3, and Worker 4 are all doing CPU-heavy computation:

> _Worker 1 → CPU-heavy task  
> Worker 2 → CPU-heavy task  
> Worker 3 → CPU-heavy task  
> Worker 4 → CPU-heavy task_

and if we have four CPU cores, those workers may actually execute at the same time:

> _Core 1 → Worker 1  
> Core 2 → Worker 2  
> Core 3 → Worker 3  
> Core 4 → Worker 4_

With one core, they can’t all execute at the same time. They have to share that execution resource.

> So concurrency doesn’t give ForgeQueue more CPU power.

It gives ForgeQueue multiple independent pieces of work that can be in progress and whether those pieces can actually execute simultaneously depends on the execution resources available.

That distinction became important when I started wondering about something else.

## Now who actually decides which worker runs?

Suppose Worker 1 is waiting for an HTTP response while Worker 2 is doing CPU work.

Then the response for Worker 1 arrives , worker 1 can continue.

**But does Worker 1 immediately take the CPU away from Worker 2?**

welll….. Not necessarily.

This was another thing I initially pictured incorrectly, I was thinking about it almost like the workers were manually passing the CPU between themselves:

Press enter or click to view image in full size

![](https://miro.medium.com/v2/resize:fit:1400/1*teEnLkyo5ZUBbbh2SzLWXg.png)

When the operation Worker 1 was waiting for completes, the goroutine can become runnable again since the Go runtime scheduler is responsible for scheduling runnable goroutines onto the available execution resources.

A simplified view is:

> _Goroutines → running / waiting / runnable → Go scheduler → execution resources_

A goroutine that is running can also be **Anticipated** so other runnable work gets execution time.

The important part for ForgeQueue is that I don’t manually decide:

> _run Worker 1 → then stop it → then run Worker 2 → then switch back to Worker 1_

but the runtime handles that scheduling.

The exact details of the Go scheduler go much deeper than I need to get into here so we will get into it on some another day.

For the worker pool, the useful mental model is simply:

> **workers are goroutines, goroutines can be running, waiting, or runnable, and the Go runtime schedules runnable goroutines onto the execution resources available to the program.**

## A very important thing that we need to understand :

### A worker being there doesn’t mean it’s using the CPU

This sounds obvious now, but it was an important distinction for me.

[](https://medium.com/blog/newsletter?source=promotion_paragraph---post_body_banner_beneficial_intelligence_nl--cb4b56c47959-----------------------------------------)

Suppose:

> _Worker 1 → waiting for Redis  
> Worker 2 → doing CPU work  
> Worker 3 → waiting for database_

It would be wrong to imagine all three workers as continuously consuming CPU time.

- Worker 1 is waiting.
- Worker 3 is waiting.
- Worker 2 can use the available execution resource.

When Worker 1’s Redis operation completes, it can become runnable again and continue.

So instead of thinking:

> _→Worker 1 is always running.  
> →Worker 2 is always running.  
> →Worker 3 is always running._

### I find this model more useful:

> _Worker 1 → running / waiting / runnable  
> Worker 2 → running / waiting / runnable  
> Worker 3 → running / waiting / runnable_

As their states can change independently and that’s the important part.

## Coming back to the worker-pool code

With this mental model, the `Start()` code made much more sense:

Press enter or click to view image in full size

![](https://miro.medium.com/v2/resize:fit:1400/1*x5g6CpAJXXhQSm2OTfpICA.png)

The `for` loop controls how many workers we create.

If:

wp.concurrencyLimit = 10

then we create ten goroutines.

But that number does **not** obviously mean L

_Give ForgeQueue ten CPU cores lol._

It means:

> _Create ten independent workers that can have work in progress._

### Those are very different things.

There are also two other pieces of this code that matter for the worker lifecycle.

First:

wp.wg.Add(1)

`wg` is a `sync.WaitGroup`.

Every time we’re about to start a worker, we tell the `WaitGroup` that another goroutine needs to be accounted for.

So if we start ten workers:

WaitGroup counter = 10

Then each worker has:

defer wp.wg.Done()

`Done()` decreases the counter when that worker finishes.

The `defer` matters because we want `Done()` to happen when the goroutine's function returns, regardless of where that return happens.

So the lifecycle is roughly:

![](https://miro.medium.com/v2/resize:fit:1344/1*7jnmpBL8xxQfv0PKyn1a9A.png)

Then `Stop()` can do:

func (wp WorkerPool) Stop() {  
    close(wp.quit)  
    wp.cancel()  
    wp.wg.Wait()  
}

Closing `wp.quit` tells the workers that shutdown has been requested. The workers eventually return from `workerLoop()`.

When each one returns, its deferred:

wp.wg.Done()

runs.

And:

wp.wg.Wait()

waits until all those workers have finished.

So here the worker pool is mainly responsible for things like:

- how many workers exist
- how much work can be active
- when workers should shut down
- waiting for workers to finish

It is **not** responsible for deciding which CPU core a worker gets.

## What does `concurrencyLimit` actually limit?

This became much easier to reason about once I separated the queue from the worker pool.

Imagine Redis has 1,000 tasks waiting:

![](https://miro.medium.com/v2/resize:fit:1344/1*rOTSmbJe4yq-EO1Y-UWCtA.png)

The fact that there are 1,000 tasks doesn’t mean I should create 1,000 workers.

If my worker pool has:

concurrencyLimit = 10

then I have ten workers.

The remaining tasks can stay in **Redis** until workers become available.

So there are really two different questions here.

### **What work is waiting?**

— →well that’s the queue.

### **How much work are we willing to have in progress at once?**

— ->well that’s the worker pool.

**That’s bounded concurrency.**

And that limit matters because active work consumes resources.

If I let an unlimited number of tasks become active, I could put unnecessary pressure on: CPU , memory , Redis obviously ,database connections , network connections, external services

A queue can therefore be very large without requiring the system to actively process every task at the same time.

That distinction is probably more useful for ForgeQueue than the abstract definition of concurrency itself.

## The worker pool and the CPU are soemwhat different layers

This was probably the most useful distinction I got from all of this.

The ForgeQueue worker pool decides how much work the application is willing to have active while the Go runtime decides which runnable goroutines get execution time.

The operating system and hardware provide the execution resources.

So, roughly:

> ForgeQueue worker pool →How much work should be active?

Once I looked at it this way, it became obvious that the worker pool shouldn’t try to implement CPU scheduling since its not thier **JOB.**

## So what does concurrency actually give ForgeQueue?

Going back to the question I started with, I think the useful answer is now pretty simple.

Suppose ForgeQueue has four tasks:

Task A  
Task B  
Task C  
Task D

At some point, they might look like this:

Task A --> waiting for Redis  
Task B --> processing data  
Task C --> waiting for database  
Task D --> processing data

They don’t all have to move together. While Task A is waiting for Redis, Task B can keep running. Task C might be waiting on the database while Task D is doing some computation.

And if there are multiple execution resources available, some of that work can actually run at the same time.

This is where I was getting concurrency and parallelism mixed up.

- **Concurrency** means multiple tasks can be making progress independently.
- **Parallelism** means multiple pieces of work are actually running at the same time.

> **So you can have concurrency even with one CPU core. With multiple cores, that concurrent work can also run in parallel.**

For ForgeQueue, this means I don’t have to think of a worker as something that’s constantly running.

**A worker can pick up a task, do some work, wait for Redis, become runnable again, continue where it left off, and eventually finish.**

While it’s waiting, another worker can be doing something else.

**The worker pool** is basically the limit on how much work I want my project to have in flight at once as go handles scheduling the goroutines

After going through all this it all kinda made sense to me as it’s all really just a way of letting multiple pieces of work move independently, without letting the queue create an unlimited number of them.

do checkout my previous blog to get more better gist on what we are building.

[

## Understanding gRPC and Protocol Buffers While Building ForgeQueue

### When I first came across Protocol Buffers and gRPC, they felt like one of those technologies everyone recommended but…

medium.com



](https://medium.com/@asyncyash/understanding-grpc-and-protocol-buffers-while-building-forgequeue-480aeffb3811?source=post_page-----cb4b56c47959-----------------------------------------)

also github code:

[

## FORGEqueue/internal/worker/worker.go at main · Yashbhu/FORGEqueue

### Contribute to Yashbhu/FORGEqueue development by creating an account on GitHub.

github.com



](https://github.com/Yashbhu/FORGEqueue/blob/main/internal/worker/worker.go?source=post_page-----cb4b56c47959-----------------------------------------)

_my documented notes while building this_ [https://yaxhmeh.vercel.app/](https://yaxhmeh.vercel.app/)