// inventory: e264_mcp_rhs_not_const
var y;
varexo e;
parameters rho;
rho = 0.9;
model;
[mcp = 'y > rho'] y = rho * y(-1) + e;
end;
