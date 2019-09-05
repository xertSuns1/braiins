# Copyright (C) 2019  Braiins Systems s.r.o.
#
# This file is part of Braiins Open-Source Initiative (BOSI).
#
# BOSI is free software: you can redistribute it and/or modify
# it under the terms of the GNU General Public License as published by
# the Free Software Foundation, either version 3 of the License, or
# (at your option) any later version.
#
# This program is distributed in the hope that it will be useful,
# but WITHOUT ANY WARRANTY; without even the implied warranty of
# MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
# GNU General Public License for more details.
#
# You should have received a copy of the GNU General Public License
# along with this program.  If not, see <https://www.gnu.org/licenses/>.
#
# Please, keep in mind that we may also license BOSI or any part thereof
# under a proprietary license. For more information on the terms and conditions
# of such proprietary license or if you have any other questions, please
# contact us at opensource@braiins.com.

include $(TOPDIR)/rules.mk

PKG_NAME:=bmminer
PKG_VERSION:=1.0.0
PKG_RELEASE:=1

PKG_MAINTAINER:=Braiins <braiins@braiins.com>

ifeq ($(strip $(CONFIG_EXTERNAL_BMMINER_TREE)),"")
	PKG_SOURCE_PROTO:=git
	PKG_SOURCE_URL:=git@gitlab.bo:x/bmminer.git
	PKG_SOURCE_VERSION:=master
else
	PKG_SOURCE_VERSION:=local
    USE_SOURCE_DIR:=$(call qstrip,$(CONFIG_EXTERNAL_BMMINER_TREE))
endif

PKG_SOURCE_SUBDIR:=$(PKG_NAME)-$(PKG_SOURCE_VERSION)
#PKG_SOURCE:=$(PKG_SOURCE_SUBDIR).tar.xz
#PKG_MIRROR_HASH:=c6d4f3a6d936dbf9ee5e96f5e843f1127c2867455250100288dc477f646db001
PKG_BUILD_DIR:=$(BUILD_DIR)/$(PKG_SOURCE_SUBDIR)

include $(INCLUDE_DIR)/package.mk

define Package/bmminer
  SECTION:=utils
  CATEGORY:=Utilities
  TITLE:=ASIC and FPGA Miner for Bitcoin
  URL:=https://github.com/bitmaintech/bmminer-mix
  DEPENDS:=+zlib
endef

define Package/bmminer/description
 Multi-threaded multi-pool FPGA and ASIC miner for bitcoin.
endef

define Package/bmminer/config
	source "$(SOURCE)/Config.in"
endef

define Build/Configure
	( cd $(PKG_BUILD_DIR); ./setminertype S9 )
endef

define Build/Compile
	+$(MAKE) CROSS_PREFIX=$(TARGET_CROSS) -C $(PKG_BUILD_DIR)
endef

define Package/bmminer/conffiles
/etc/bmminer.conf
endef

define Package/bmminer/install
	$(INSTALL_DIR) $(1)/usr/bin
	$(INSTALL_BIN) $(PKG_BUILD_DIR)/bmminer $(1)/usr/bin/
	$(INSTALL_BIN) ./files/bmminer.sh $(1)/usr/bin/

	$(INSTALL_DIR) $(1)/etc
	$(INSTALL_BIN) ./files/bmminer.conf $(1)/etc/

	$(INSTALL_DIR) $(1)/etc/bmminer
	$(INSTALL_BIN) ./files/hash_s8_app.txt $(1)/etc/bmminer/
	$(INSTALL_BIN) ./files/dsPIC33EP16GS202_app.txt $(1)/etc/bmminer/
endef

$(eval $(call BuildPackage,bmminer))
