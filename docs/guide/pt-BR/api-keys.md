---
id: api-keys
title: Suas chaves de API
group: getting-started
place: settings/keys
---

# Suas chaves de API

O Bardo não tem IA própria nem conta para criar. Ele trabalha com as suas contas em cada provedor, por meio de chaves de API que você cria lá e salva uma única vez em [Configurações › Chaves de API](bardo:go/settings/keys). Você paga cada provedor diretamente, pelos preços dele.

<a id="providers"></a>
## O que cada provedor faz

- **Claude** escreve roteiros, títulos, descrições, ideias de vídeo, planos de cena e os prompts para os modelos de imagem e vídeo. Você precisa dele para o roteiro, as cenas, as ideias e os textos dos posts.
- **ElevenLabs** narra e lista as suas vozes, clones incluídos. Você precisa dela para a narração e as vozes das personas.
- **Gemini** gera as imagens das cenas (Nano Banana) e clipes de vídeo (Veo, Gemini Omni Flash). Você precisa dele para as imagens das cenas e, para os clipes, só se usar os modelos do Google.
- **Higgsfield** gera clipes de vídeo pelos modelos que oferece. Você só precisa dela para clipes, se escolher os modelos dela.
- **TypeSafe (JEV)** é o motor de decisão: ranqueia ideias de vídeo e pontua sugestões de corte. Você precisa dele para ranquear ideias e sugerir cortes.
- **YouTube Data API** traz a pesquisa de nicho e as estatísticas públicas dos vídeos. Você precisa dela para a pesquisa e para os números públicos dos seus posts.

Você não precisa de todas as chaves no primeiro dia. Uma tela que depende de uma chave que falta avisa. Clipes são opcionais: uma cena sem clipe fica com a imagem parada.

<a id="where-kept"></a>
## Onde as chaves ficam

As chaves ficam no Gerenciador de Credenciais do Windows, na sua conta do Windows, nunca no banco de dados do Bardo. Sempre que um texto sai do app (logs, mensagens de erro, a tela), a chave aparece mascarada, e as Configurações mostram só os últimos caracteres.

Remover uma chave nas Configurações apaga ela do Gerenciador de Credenciais. Para desativar a chave de vez, revogue também no painel do próprio provedor.

<a id="testing"></a>
## Testar uma chave

**Testar chave** faz a chamada mais barata que o provedor oferece e que exige uma chave válida, e diz o que encontrou: a chave funciona, o provedor recusou, falta uma permissão ou uma API ativada, ou uma cota ou falta de créditos está bloqueando agora. Um teste da YouTube Data API usa 1 unidade da sua cota diária.

<a id="budgets"></a>
## Custos e orçamentos

Cada geração registra quanto custou, pelo valor que o provedor informou ou estimado pela tabela de preços dele. Em [Custos](bardo:go/costs) você vê quanto gastou no mês, por provedor, por canal e por vídeo.

Defina um **orçamento** mensal para cada provedor na tela Custos. Aos 80% a navegação avisa, e quando um orçamento é atingido, cada tarefa nova desse provedor pergunta antes de começar. Nada é interrompido no meio.
