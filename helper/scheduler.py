# -*- coding: utf-8 -*-
"""
-------------------------------------------------
   File Name：     proxyScheduler
   Description :
   Author :        JHao
   date：          2019/8/5
-------------------------------------------------
   Change Activity:
                   2019/08/05: proxyScheduler
                   2021/02/23: runProxyCheck时,剩余代理少于POOL_SIZE_MIN时执行抓取
                   2026/11/16: 采集预过滤(已在池中不重复校验)/刚入库免复检/采集与check非阻塞互斥
-------------------------------------------------
"""
__author__ = "JHao"

from threading import Lock
from apscheduler.schedulers.blocking import BlockingScheduler
from apscheduler.executors.pool import ProcessPoolExecutor

from util.six import Queue
from helper.fetch import Fetcher
from helper.check import Checker
from handler.logHandler import LogHandler
from handler.proxyHandler import ProxyHandler
from handler.configHandler import ConfigHandler

# 采集互斥: 定时抓取任务与check内嵌补采共用, 同一时刻只允许一轮采集
# (防并发双跑造成重复抓取/校验; 获取频率本身不做限制)
__fetch_lock = Lock()

# check互斥: 上轮全量校验未结束时跳过本轮, 不让多轮check并存互相挤占
__check_lock = Lock()

__log = LogHandler("scheduler")


def __runProxyFetch():
    """
    采集 + raw校验
    :return: 本轮新入库的代理key集合(set), 供check轮跳过刚入库代理的复检
    """
    if not __fetch_lock.acquire(blocking=False):
        __log.info("fetch: 已有采集任务在执行, 本轮跳过")
        return set()

    try:
        proxy_queue = Queue()
        proxy_fetcher = Fetcher()
        proxy_handler = ProxyHandler()

        # 预过滤: 已在池中的代理跳过raw校验(use周期任务会覆盖它们),
        # 否则池内常驻代理每轮抓取都被重复验证一次
        existing = set(proxy_handler.getAllKeys())
        fetched = 0
        to_validate = 0
        for proxy in proxy_fetcher.run():
            fetched += 1
            if proxy.proxy not in existing:
                proxy_queue.put(proxy)
                to_validate += 1
        __log.info("fetch: fetched %s, already in pool %s, to validate %s" %
                   (fetched, fetched - to_validate, to_validate))

        return set(Checker("raw", proxy_queue))
    finally:
        __fetch_lock.release()


def __runProxyCheck():
    if not __check_lock.acquire(blocking=False):
        __log.info("check: 上一轮校验尚未完成, 本轮跳过")
        return

    try:
        proxy_handler = ProxyHandler()
        proxy_queue = Queue()
        if proxy_handler.db.getCount().get("total", 0) < proxy_handler.conf.poolSizeMin:
            # 池子低于下限: 先补采(共用采集互斥), 拿到本轮新入库的key
            ingested = __runProxyFetch()
        else:
            ingested = set()

        for proxy in proxy_handler.getAll():
            # 刚入库的代理刚通过raw校验, 本轮免复检
            if proxy.proxy in ingested:
                continue
            proxy_queue.put(proxy)

        Checker("use", proxy_queue)
    finally:
        __check_lock.release()


def runScheduler():
    __runProxyFetch()

    timezone = ConfigHandler().timezone
    scheduler_log = LogHandler("scheduler")
    scheduler = BlockingScheduler(logger=scheduler_log, timezone=timezone)

    scheduler.add_job(
        __runProxyFetch, "interval", minutes=10, id="proxy_fetch", name="proxy采集"
    )
    scheduler.add_job(
        __runProxyCheck, "interval", minutes=2, id="proxy_check", name="proxy检查"
    )
    executors = {
        "default": {"type": "threadpool", "max_workers": 20},
        "processpool": ProcessPoolExecutor(max_workers=5),
    }
    job_defaults = {"coalesce": False, "max_instances": 10}

    scheduler.configure(
        executors=executors, job_defaults=job_defaults, timezone=timezone
    )

    scheduler.start()


if __name__ == "__main__":
    runScheduler()
