// inventory: e262_mcp_lhs_not_var
var y;
varexo e;
parameters rho;
rho = 0.9;
model;
[mcp = '0 > 0'] y = rho * y(-1) + e;
end;
