# -*- coding: utf-8 -*-
"""
-------------------------------------------------
   File Name：     WebRequest
   Description :   Network Requests Class
   Author :        J_hao
   date：          2017/7/31
-------------------------------------------------
   Change Activity:
                   2017/7/31:
-------------------------------------------------
"""
__author__ = "J_hao"

from requests.models import Response
from lxml import etree
import requests
import random
import time

from handler.logHandler import LogHandler
from handler.configHandler import ConfigHandler

requests.packages.urllib3.disable_warnings()


class WebRequest(object):
    name = "web_request"

    def __init__(self, *args, **kwargs):
        self.log = LogHandler(self.name, file=False)
        self.conf = ConfigHandler()
        self.response = Response()

    @property
    def user_agent(self):
        """
        return an User-Agent at random
        :return:
        """
        ua_list = [
            "Mozilla/5.0 (Windows NT 6.1; WOW64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/30.0.1599.101",
            "Mozilla/5.0 (Windows NT 6.1; WOW64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/38.0.2125.122",
            "Mozilla/5.0 (Windows NT 6.1; WOW64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/39.0.2171.71",
            "Mozilla/5.0 (Windows NT 6.1; WOW64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/39.0.2171.95",
            "Mozilla/5.0 (Windows NT 6.1; WOW64) AppleWebKit/537.1 (KHTML, like Gecko) Chrome/21.0.1180.71",
            "Mozilla/4.0 (compatible; MSIE 6.0; Windows NT 5.1; SV1; QQDownload 732; .NET4.0C; .NET4.0E)",
            "Mozilla/5.0 (Windows NT 5.1; U; en; rv:1.8.1) Gecko/20061208 Firefox/2.0.0 Opera 9.50",
            "Mozilla/5.0 (Windows NT 6.1; WOW64; rv:34.0) Gecko/20100101 Firefox/34.0",
        ]
        return random.choice(ua_list)

    @property
    def header(self):
        """
        basic header
        :return:
        """
        return {
            "User-Agent": self.user_agent,
            "Accept": "*/*",
            "Connection": "keep-alive",
            "Accept-Language": "zh-CN,zh;q=0.8",
        }

    def get(
        self,
        url,
        header=None,
        retry_time=3,
        retry_interval=5,
        timeout=5,
        params=None,
        proxy=None,
        *args,
        **kwargs
    ):
        """
        get method
        :param url: target url
        :param header: headers
        :param retry_time: retry time
        :param retry_interval: retry interval
        :param timeout: network timeout
        :param proxy: 指定本次请求使用的代理(如 http://ip:port),
                      默认直连, 直连不通时自动改用 setting.FETCH_PROXY 重试
        :return:
        """
        headers = self.header
        if header and isinstance(header, dict):
            headers.update(header)
        fetch_proxy = proxy or self.conf.fetchProxy
        # 先直连(只试一次,快速失败), 直连不通且配置了抓取代理时, 通过代理按retry_time重试
        if self.__request(
            url, headers, params, timeout, None,
            1 if fetch_proxy else retry_time, retry_interval, *args, **kwargs
        ):
            return self
        if fetch_proxy and self.__request(
            url, headers, params, timeout,
            {"http": fetch_proxy, "https": fetch_proxy},
            retry_time, retry_interval, *args, **kwargs
        ):
            return self
        resp = Response()
        resp.status_code = 200
        return self

    def __request(
        self, url, headers, params, timeout, proxies, retry_time, retry_interval, *args, **kwargs
    ):
        """
        send get request with proxies
        :return: True if status code is 200 else False
        """
        while True:
            try:
                self.response = requests.get(
                    url,
                    headers=headers,
                    params=params,
                    timeout=timeout,
                    proxies=proxies,
                    *args,
                    **kwargs
                )
                if self.response.status_code == 200:
                    return True
                self.log.error(
                    "requests: %s error: status_code %s"
                    % (url, self.response.status_code)
                )
            except Exception as e:
                self.log.error("requests: %s error: %s" % (url, str(e)))
            retry_time -= 1
            if retry_time <= 0:
                return False
            self.log.info("retry %s second after" % retry_interval)
            time.sleep(retry_interval)

    @property
    def tree(self):
        return etree.HTML(self.response.content)

    @property
    def text(self):
        return self.response.text

    @property
    def json(self):
        try:
            return self.response.json()
        except Exception as e:
            self.log.error(str(e))
            return {}
