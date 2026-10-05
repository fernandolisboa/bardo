---
id: background-agent
title: Publicar com o Bardo fechado
group: publishing
place: settings/publishing
tour: background-agent
---

# Publicar com o Bardo fechado

O Instagram não aceita horário de publicação vindo de apps, então o próprio Bardo posta o Reel agendado no horário marcado ([Agendamento](uploading.md#schedule)). Sozinho, isso exige o Bardo aberto nesse horário. [Configurações › Publicação](bardo:go/settings/publishing) pode adicionar um **agente em segundo plano**, leve, que envia esses posts com o Bardo fechado. Ele fica desligado até você ligar. [Mostre a aba](bardo:tour/background-agent).

<a id="what"></a>
## O que o agente envia

O agente envia o que o Bardo posta num horário marcado: Reels do Instagram. O YouTube publica sozinho os vídeos agendados, mesmo com o computador desligado, e os rascunhos do TikTok são postados pelo app do TikTok, então o agente não tem nada a fazer com eles.

Ele usa os mesmos dados, contas e chaves do Bardo: nada é copiado, e suas chaves e tokens continuam no Gerenciador de Credenciais do Windows. O envio dele aparece em [Tarefas](jobs.md) e na etapa Publicar do projeto como qualquer outro.

<a id="turn-on"></a>
## Ligar e desligar

**Publicar com o Bardo fechado** pede ao Windows para iniciar o agente sempre que você se conectar, como você mesmo, sem direitos de administrador, e já inicia o agente na hora. Desligar para o agente e o remove do Windows. Se o Windows recusar alguma dessas mudanças, o Bardo avisa: um agente que não pôde ser configurado continua desligado, e um que o Windows não deixou remover para sozinho e é removido na próxima vez que o Bardo abrir.

<a id="status"></a>
## Está rodando?

Abaixo da opção, o Bardo mostra se o agente está desligado, rodando, ou ligado mas parado agora (por exemplo, depois de um erro). O Windows inicia o agente de novo na próxima vez que você se conectar, e confere a cada 15 minutos; **Iniciar agora** inicia na hora.

<a id="limits"></a>
## Do que ele precisa

- O computador precisa estar ligado, e você conectado ao Windows. Tela bloqueada não atrapalha; o agente não acorda um computador em suspensão e não roda com você desconectado.
- Com o Bardo aberto, o próprio Bardo envia os posts e o agente espera. Cada post sai uma vez só, seja quem for que envie.
- Um post cujo horário passa sem nenhum dos dois rodando fica perdido, e o Bardo lista na próxima vez que abrir: veja [Posts perdidos](missed-posts.md).
