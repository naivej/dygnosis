// inventory: p_hank_fire_epilogue_sum
heterogeneity_dimension d;
var(heterogeneity=d) yh;
var y;
varexo e;
model;
y = e + SUM(yh);
end;
model(heterogeneity=d);
yh = yh(-1);
end;
epilogue;
foo = y + SUM(y);
end;