# -*- coding: utf-8 -*-
"""
-------------------------------------------------
   File Name：     proxyFetcher
   Description :
   Author :        JHao
   date：          2016/11/25
-------------------------------------------------
   Change Activity:
                   2016/11/25: proxyFetcher
-------------------------------------------------
"""
__author__ = "JHao"

import re
import json
from time import sleep

from util.webRequest import WebRequest


class ProxyFetcher(object):
    """
    proxy getter
    """

    @staticmethod
    def freeProxy01():
        """开心代理"""
        target_urls = [
            "http://www.kxdaili.com/dailiip.html",
            "http://www.kxdaili.com/dailiip/2/1.html",
        ]
        for url in target_urls:
            tree = WebRequest().get(url).tree
            for tr in tree.xpath("//table[@class='active']//tr")[1:]:
                ip = "".join(tr.xpath("./td[1]/text()")).strip()
                port = "".join(tr.xpath("./td[2]/text()")).strip()
                yield "%s:%s" % (ip, port)

    @staticmethod
    def freeProxy02(page_count=1):
        """快代理 https://www.kuaidaili.com"""
        url_pattern = [
            "https://www.kuaidaili.com/free/inha/{}/",
            "https://www.kuaidaili.com/free/intr/{}/",
        ]
        url_list = []
        for page_index in range(1, page_count + 1):
            for pattern in url_pattern:
                url_list.append(pattern.format(page_index))

        for url in url_list:
            tree = WebRequest().get(url).tree
            proxy_list = tree.xpath(".//table//tr")
            sleep(1)  # 必须sleep 不然第二条请求不到数据
            for tr in proxy_list[1:]:
                yield ":".join(tr.xpath("./td/text()")[0:2])

    @staticmethod
    def freeProxy03():
        """云代理"""
        urls = [
            "http://www.ip3366.net/free/?stype=1",
            "http://www.ip3366.net/free/?stype=2",
        ]
        for url in urls:
            r = WebRequest().get(url, timeout=10)
            proxies = re.findall(
                r"<td>(\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3})</td>[\s\S]*?<td>(\d+)</td>",
                r.text,
            )
            for proxy in proxies:
                yield ":".join(proxy)

    @staticmethod
    def freeProxy04():
        """89免费代理"""
        r = WebRequest().get("https://www.89ip.cn/index_1.html", timeout=10)
        proxies = re.findall(
            r"<td.*?>[\s\S]*?(\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3})[\s\S]*?</td>[\s\S]*?<td.*?>[\s\S]*?(\d+)[\s\S]*?</td>",
            r.text,
        )
        for proxy in proxies:
            yield ":".join(proxy)

    @staticmethod
    def freeProxy05():
        """稻壳代理 https://www.docip.net/"""
        r = WebRequest().get("https://www.docip.net/data/free.json", timeout=10)
        try:
            for each in r.json["data"]:
                yield each["ip"]
        except Exception as e:
            print(e)

    @staticmethod
    def freeProxy06():
        urls = ["https://list.proxylistplus.com/Fresh-HTTP-Proxy-List-1"]
        request = WebRequest()
        for url in urls:
            r = request.get(url, timeout=10)
            proxies = re.findall(
                r"<td>(\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3})</td>[\s\S]*?<td>(\d+)</td>",
                r.text,
            )
            for proxy in proxies:
                yield ":".join(proxy)

    @staticmethod
    def freeProxy07():
        r = WebRequest().get(
            "https://raw.githubusercontent.com/CharlesPikachu/freeproxy/master/proxies.json",
            timeout=10,
        )
        try:
            for each in r.json["data"]:
                if (
                    "http" in each["protocol"].lower()
                    or "https" in each["protocol"].lower()
                ):
                    ip = each["ip"]
                    port = each["port"]
                    yield "%s:%s" % (ip, port)
        except Exception as e:
            print(e)

    @staticmethod
    def freeProxy08():
        r = WebRequest().get("http://api.66daili.com/?format=json", timeout=10)
        try:
            for each in r.json["data"]:
                ip = each["ip"]
                port = each["port"]
                yield "%s:%s" % (ip, port)
        except Exception as e:
            print(e)

    @staticmethod
    def freeProxy09():
        r = WebRequest().get("http://api.goodips.com/?format=json", timeout=10)
        try:
            for each in r.json["data"]:
                if (
                    "http" in each["protocol"].lower()
                    or "https" in each["protocol"].lower()
                ):
                    ip = each["ip"]
                    port = each["port"]
                    yield "%s:%s" % (ip, port)

        except Exception as e:
            print(e)

    @staticmethod
    def freeProxy10():
        r = WebRequest().get(
            "https://cdn.jsdelivr.net/gh/proxifly/free-proxy-list@main/proxies/protocols/http/data.json",
            timeout=10,
        )
        try:
            for each in r.json:
                ip = each["ip"]
                port = each["port"]
                yield "%s:%s" % (ip, port)
        except Exception as e:
            print(e)

    @staticmethod
    def freeProxy11():
        r = WebRequest().get(
            "https://cdn.jsdelivr.net/gh/proxifly/free-proxy-list@main/proxies/protocols/https/data.json",
            timeout=10,
        )
        try:
            for each in r.json:
                ip = each["ip"]
                port = each["port"]
                yield "%s:%s" % (ip, port)
        except Exception as e:
            print(e)

    @staticmethod
    def freeProxy12():
        r = WebRequest().get(
            "https://proxylist.geonode.com/api/proxy-list?limit=100&protocols=http&page=1&sort_by=lastChecked&sort_type=desc",
            timeout=10,
        )
        try:
            for each in r.json["data"]:
                ip = each["ip"]
                port = each["port"]
                yield "%s:%s" % (ip, port)
        except Exception as e:
            print(e)

    @staticmethod
    def freeProxy13():
        r = WebRequest().get(
            "https://raw.githubusercontent.com/TheSpeedX/SOCKS-List/master/http.txt",
            timeout=10,
        )
        try:
            for line in r.text.strip().split("\n"):
                line = line.strip()
                if ":" in line:
                    parts = line.split(":")
                    if len(parts) == 2 and parts[1].isdigit():
                        ip = parts[0]
                        port = int(parts[1])
                        yield "%s:%s" % (ip, port)
        except Exception as e:
            print(e)

    @staticmethod
    def freeProxy14():
        r = WebRequest().get(
            "https://raw.githubusercontent.com/parserpp/ip_ports/main/proxyinfo.json",
            timeout=10,
        )
        try:
            for each in r.json["http_high_anonymous"]:
                ip = each["host"]
                port = each["port"]
                yield "%s:%s" % (ip, port)

        except Exception as e:
            print(e)

    @staticmethod
    def freeProxy15():
        """89IP代理"""
        headers = {
            "Accept": "text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,image/apng,*/*;q=0.8,application/signed-exchange;v=b3;q=0.7",
            "Referer": "https://www.89ip.cn/ti.html",
            "User-Agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/123.0.0.0 Safari/537.36",
        }

        r = WebRequest().get(
            "https://www.89ip.cn/tqdl.html",
            header=headers,
            params={
                "num": "9999",
                "address": "",
                "kill_address": "",
                "port": "",
                "kill_port": "",
                "isp": "",
            },
            timeout=10,
        )
        try:
            for each in re.findall(r"\b(?:\d{1,3}\.){3}\d{1,3}:\d+\b", r.text):
                yield each

        except Exception as e:
            print(e)

    @staticmethod
    def freeProxy16():
        """稻壳代理"""
        r = WebRequest().get("https://www.docip.net/data/free.json", timeout=10)
        try:
            for each in r.json["data"]:
                yield each["ip"]
        except Exception as e:
            print(e)


if __name__ == "__main__":
    p = ProxyFetcher()
    for _ in p.freeProxy16():
        print(_)

# http://nntime.com/proxy-list-01.htm
