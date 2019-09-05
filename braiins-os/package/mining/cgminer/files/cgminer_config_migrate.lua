#!/usr/bin/env lua

-- Copyright (C) 2019  Braiins Systems s.r.o.
--
-- This file is part of Braiins Open-Source Initiative (BOSI).
--
-- BOSI is free software: you can redistribute it and/or modify
-- it under the terms of the GNU General Public License as published by
-- the Free Software Foundation, either version 3 of the License, or
-- (at your option) any later version.
--
-- This program is distributed in the hope that it will be useful,
-- but WITHOUT ANY WARRANTY; without even the implied warranty of
-- MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
-- GNU General Public License for more details.
--
-- You should have received a copy of the GNU General Public License
-- along with this program.  If not, see <https://www.gnu.org/licenses/>.
--
-- Please, keep in mind that we may also license BOSI or any part thereof
-- under a proprietary license. For more information on the terms and conditions
-- of such proprietary license or if you have any other questions, please
-- contact us at opensource@braiins.com.

require 'luci.jsonc'
require 'nixio.fs'

local CGMINER_CONFIG = '/etc/cgminer.conf'

local function migrate_am1(cfg)
	local ver = tonumber(cfg['config-format-revision'])
	if not ver then
		-- upgrade from prehistoric to version 1
		for i = 1, 6 do
			cfg[('A1Pll%d'):format(i)] = nil
		end
		cfg.A1Vol = nil
		cfg['enabled-chains'] = nil
		cfg['bitmain-voltage'] = nil
		-- enable multi-version if upgrading
		cfg['multi-version'] = '4'
		-- update config format revision
		cfg['config-format-revision'] = '1'
		return true
	end
	return false
end

local function migrate_dm1(cfg)
	local ver = tonumber(cfg['config-format-revision'])
	if not ver then
		-- upgrade from prehistoric to version 1
		cfg['bitmain-use-vil'] = nil
		cfg['bitmain-freq'] = nil
		cfg['no-pre-heat'] = nil
		cfg['bitmain-voltage'] = nil
		cfg['multi-version'] = nil
		cfg['fixed-freq'] = nil
		cfg['config-format-revision'] = '1'
		return true
	end
	return false
end

-- migration function takes "json" argument
-- returns true if config was migrated, false if it is the latest revision
migrate_fns = {
	['am1-s9'] = migrate_am1,
	['dm1-g9'] = migrate_dm1,
	['dm1-g19'] = migrate_dm1,
	['dm1-g29'] = migrate_dm1,
}

function main(arg)
	local path = arg[1]
	local platform = arg[2]
	if not path or not platform then
		io.stderr:write('usage: cgminer_config_migrate <config_file> <platform>\n')
		os.exit(1)
	end
	local migrate = migrate_fns[platform]
	if not migrate then
		return
	end
	local str = nixio.fs.readfile(path)
	if not str then error('file not found') end
	cfg = luci.jsonc.parse(str)
	if not cfg then error('json parser failed') end
	
	if migrate(cfg) then
		print('config '..path..' was migrated')
		nixio.fs.writefile(path, luci.jsonc.stringify(cfg))
	end
end

main(arg)
