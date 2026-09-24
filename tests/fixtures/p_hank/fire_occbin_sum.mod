// inventory: p_hank_fire_occbin_sum
heterogeneity_dimension d;
var(heterogeneity=d) yh;
var y, i;
varexo e;
model;
y = e + SUM(yh);
[name='policy', bind='ELB'] i = e;
[name='policy', relax='ELB'] i = 1*e;
end;
model(heterogeneity=d);
yh = yh(-1);
end;
occbin_constraints;
name 'ELB';
bind SUM(i) <= 0;
relax i <= 0;
end;
