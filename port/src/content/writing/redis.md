---
title: Redis
slug: forgequeue-redis
published: true
date: 2026-08-30
description: "Redis data structures powering ForgeQueue: LPUSH, sorted sets, and BRPOP."
tags:
  - Redis
readingTime: 4 min
links:
  - ForgeQueue
---
# REDIS



sorted sets A Redis sorted set is a collection of unique strings (members) ordered by an associated score. When more than one string has the same score, the strings are ordered lexicographically.

[lpush](https://yaxhmeh.vercel.app/tags/lpush)

`LPUSH key value [value ...] tr.redisClient.LPush( 	ctx, 	"queue:tasks:immediate", 	serializedData, ).Err()`

where “queue:tasks:immediate”, is key and serializedData, value we pass ctx in order to keep the lifescyle over connection for this task ![](https://yaxhmeh.vercel.app/screenshot-2026-06-16-at-2.34.35-am.png) ![](https://yaxhmeh.vercel.app/screenshot-2026-06-18-at-2.34.02-am.png) task are best when something is asap where task arrives and execution is asap for ex

- Send email
- Resize image
- Generate PDF
- Push notification

[zset](https://yaxhmeh.vercel.app/tags/zset)

`redis.Z{ 	Score: float64(targetTime), 	Member: serializedData, }`

score = when to run Member: serialised data ![](https://yaxhmeh.vercel.app/screenshot-2026-06-18-at-2.38.42-am.png) why float 64 ? the driver expects float 64 so we convert the target time which is int64 by float64(int64) DIAGRAM EXPLAINING THE PROCESS WHERE ROUTETASK IS A FUNCTION TAKING PARAMETERS AS CTX ,ID , TASKTYPE,PAYLOAD,MAXTRIES![](https://yaxhmeh.vercel.app/screenshot-2026-06-18-at-2.44.34-pm.png)

![](https://yaxhmeh.vercel.app/screenshot-2026-06-18-at-2.48.50-am.png) If you write a standard loop that checks Redis for new items, it runs hundreds of thousands of times per second. If the queue is empty, your worker is just spinning in place, burning CPU cycles for no reason. This is called a **busy-wait trap**.

To fix this, we use a blocking Redis command called **`BRPop`** (Blocking Right Pop).

Instead of asking Redis _“Got anything now? How about now? How about now?”_, `BRPop` tells Redis: _“Hey, look at the right side of this list. If there is a task, give it to me immediately. If there isn’t, I am going to sleep right here for 2 seconds. Wake me up the exact millisecond a task drops in, or cut me loose when the 2 seconds expire.”_

type WorkerPool struct { redisClient *redis.Client }

func NewWorkerPool(client *redis.Client, …) *WorkerPool { return &WorkerPool{ redisClient: client, } } dependency injection

[brpop](https://yaxhmeh.vercel.app/tags/brpop)