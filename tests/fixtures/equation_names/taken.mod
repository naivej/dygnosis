heterogeneity_dimension d;
var y;
var(heterogeneity=d) c;
@#define is = 1:2
model;
@#for i in is
[name='eq1']
y = y(-1);
@#endfor
end;
model(heterogeneity=d);
c = c(-1);
end;
