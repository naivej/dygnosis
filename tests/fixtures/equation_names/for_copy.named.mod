var y;
@#define is = 1:2
model;
[name='eq1'] y = y(-1);
@#for i in is
y = y(-1);
@#endfor
end;
